//! LeapCheck engine CLI. See docs/07-interfaces.md for the contract.

mod db;
mod detect;
mod model;
mod php;
mod trace;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use model::{Analyzer, Component, Finding, Requirement};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_ZIP_BYTES: u64 = 80 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 400 * 1024 * 1024;

#[derive(Parser)]
#[command(name = "leapcheck", version, about = "Check before you leap: upgrade-readiness scanner")]
struct Cli {
    /// SQLite database path
    #[arg(long, global = true, default_value = "data/leapcheck.sqlite")]
    db: String,
    /// Directory holding detection rules
    #[arg(long, global = true, default_value = "engine/rules")]
    rules: PathBuf,
    /// Directory holding the PHP tools (composer install)
    #[arg(long, global = true, default_value = "tools/php")]
    php_tools: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download a WordPress.org plugin/theme and scan it
    ScanWp {
        #[arg(long)]
        slug: String,
        /// Version to scan (default: latest)
        #[arg(long)]
        version: Option<String>,
        #[arg(long, default_value = "plugin")]
        kind: String,
        /// Target PHP version(s), repeatable or comma separated, e.g. 8.3,8.4,8.5
        #[arg(long, value_delimiter = ',', default_value = "8.4")]
        target: Vec<String>,
    },
    /// Scan a local directory
    ScanDir {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        version: String,
        #[arg(long, default_value = "local")]
        ecosystem: String,
        #[arg(long, value_delimiter = ',', default_value = "8.4")]
        target: Vec<String>,
    },
    /// Export static JSON result files
    Export {
        #[arg(long, default_value = "public")]
        out: PathBuf,
    },
    /// List runs, or show one run's full trace
    Runs {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        failed: bool,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Health report over the last N hours (JSON)
    Health {
        #[arg(long, default_value_t = 24)]
        hours: u32,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("{}", json!({"level": "error", "msg": format!("{e:#}")}));
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let conn = db::open(&cli.db)?;
    match &cli.cmd {
        Cmd::ScanWp { slug, version, kind, target } => {
            let eco = match kind.as_str() {
                "plugin" => "wordpress-plugin",
                "theme" => "wordpress-theme",
                _ => bail!("--kind must be plugin or theme"),
            };
            if !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                bail!("invalid slug");
            }
            let run = trace::Run::start(&conn, "scan_package", json!({"ecosystem": eco, "slug": slug, "version": version, "targets": target}))?;
            let res = scan_wp(&cli, &conn, &run, eco, slug, version.clone(), target);
            run.finish(&res)?;
            res
        }
        Cmd::ScanDir { path, name, version, ecosystem, target } => {
            let run = trace::Run::start(&conn, "scan_dir", json!({"ecosystem": ecosystem, "name": name, "version": version, "path": path, "targets": target}))?;
            let res = scan_root(&cli, &conn, &run, ecosystem, name, version, path, target);
            run.finish(&res)?;
            res
        }
        Cmd::Export { out } => {
            let run = trace::Run::start(&conn, "export", json!({"out": out}))?;
            let res = run.step("export", || export(&conn, out));
            run.finish(&res.as_ref().map(|_| ()).map_err(|e| anyhow::anyhow!("{e:#}")))?;
            let n = res?;
            println!("{}", json!({"exported_files": n, "out": out}));
            Ok(())
        }
        Cmd::Runs { id, failed, limit } => show_runs(&conn, id.clone(), *failed, *limit),
        Cmd::Health { hours } => health(&conn, *hours),
    }
}

// ---------- WordPress.org fetch ----------

/// HTTP client: honours HTTPS_PROXY/HTTP_PROXY and the system certificate store.
fn http() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout(std::time::Duration::from_secs(120))
        .user_agent(concat!("leapcheck/", env!("CARGO_PKG_VERSION"), " (+https://github.com/agkartikey575/leapcheck)"))
        .build()
}

fn latest_version(eco: &str, slug: &str) -> Result<String> {
    let url = if eco == "wordpress-plugin" {
        format!("https://api.wordpress.org/plugins/info/1.2/?action=plugin_information&request[slug]={slug}&request[fields][sections]=0")
    } else {
        format!("https://api.wordpress.org/themes/info/1.2/?action=theme_information&request[slug]={slug}&request[fields][sections]=0")
    };
    let v: serde_json::Value = serde_json::from_str(&http().get(&url).call()?.into_string()?)?;
    if let Some(e) = v.get("error") {
        bail!("WordPress.org: {e}");
    }
    v.get("version").and_then(|x| x.as_str()).map(String::from).context("no version in API response")
}

fn download(eco: &str, slug: &str, version: &str, dest: &Path) -> Result<Vec<u8>> {
    let url = if eco == "wordpress-plugin" {
        format!("https://downloads.wordpress.org/plugin/{slug}.{version}.zip")
    } else {
        format!("https://downloads.wordpress.org/theme/{slug}.{version}.zip")
    };
    let resp = http().get(&url).call().with_context(|| format!("GET {url}"))?;
    let mut buf = vec![];
    resp.into_reader().take(MAX_ZIP_BYTES + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_ZIP_BYTES {
        bail!("zip larger than {} MB", MAX_ZIP_BYTES / 1024 / 1024);
    }
    std::fs::create_dir_all(dest)?;
    Ok(buf)
}

/// Safe unzip: rejects path traversal (zip-slip) and caps total size. Code is never executed.
fn unzip(bytes: &[u8], dest: &Path) -> Result<u64> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let Some(rel) = f.enclosed_name() else { bail!("unsafe path in zip: {}", f.name()) };
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        total += f.size();
        if total > MAX_UNPACKED_BYTES {
            bail!("unpacked size over limit");
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut w = std::fs::File::create(&out)?;
        std::io::copy(&mut f, &mut w)?;
    }
    Ok(total)
}

fn scan_wp(cli: &Cli, conn: &Connection, run: &trace::Run, eco: &str, slug: &str, version: Option<String>, targets: &[String]) -> Result<()> {
    let version = match version {
        Some(v) => v,
        None => run.step("resolve_version", || latest_version(eco, slug))?,
    };
    run.note("resolve_version", &format!("{slug}@{version}"))?;
    let work = std::env::temp_dir().join(format!("leapcheck-{}", run.id));
    let bytes = run.step("fetch", || download(eco, slug, &version, &work))?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    run.set_meta(Some(&hash), None)?;
    let res = (|| {
        run.step("unpack", || unzip(&bytes, &work))?;
        // Zips contain a top-level folder named after the slug.
        let root = if work.join(slug).is_dir() { work.join(slug) } else { work.clone() };
        scan_root(cli, conn, run, eco, slug, &version, &root, targets)
    })();
    let _ = std::fs::remove_dir_all(&work);
    res
}

fn count_encoded(root: &Path) -> u32 {
    let mut n = 0;
    for e in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        if e.path().extension().map_or(false, |x| x == "php") {
            if let Ok(b) = std::fs::read(e.path()) {
                let h = String::from_utf8_lossy(&b[..b.len().min(4096)]);
                if h.contains("ioncube") || h.contains("ionCube") || h.contains("sg_load(") || h.contains("SourceGuardian") {
                    n += 1;
                }
            }
        }
    }
    n
}

#[allow(clippy::too_many_arguments)]
fn scan_root(cli: &Cli, conn: &Connection, run: &trace::Run, eco: &str, name: &str, version: &str, root: &Path, targets: &[String]) -> Result<()> {
    if !root.is_dir() {
        bail!("not a directory: {}", root.display());
    }
    let rules = run.step("load_rules", || detect::Rules::load(&cli.rules))?;
    run.note("load_rules", &rules.summary())?;
    let analyzer = run.step("locate_analyzer", || php::PhpCompatibility::locate(&cli.php_tools))?;
    run.set_meta(None, Some(&format!("{} | {}", analyzer.id(), analyzer.version())))?;

    let components = run.step("detect_components", || Ok(detect::components(root, &rules)))?;
    let requirements = run.step("detect_requirements", || Ok(detect::requirements(root, &rules)))?;
    let encoded = count_encoded(root);
    if encoded > 0 {
        run.note("detect_encoded", &format!("{encoded} encoded files cannot be scanned"))?;
    }

    let mut per_target = vec![];
    for t in targets {
        let mut findings = run.step(&format!("analyze:php:{t}"), || analyzer.analyze(root, t))?;
        for f in findings.iter_mut() {
            f.component = detect::attribute(&f.file, &components);
        }
        per_target.push((t.clone(), findings));
    }
    run.step("store", || store(conn, &run.id, eco, name, version, &components, &requirements, &per_target, encoded))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn store(conn: &Connection, run_id: &str, eco: &str, name: &str, version: &str, comps: &[Component], reqs: &[Requirement], per_target: &[(String, Vec<Finding>)], encoded: u32) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("INSERT OR IGNORE INTO package_version (ecosystem, name, version) VALUES (?1, ?2, ?3)", params![eco, name, version])?;
    let pv: i64 = tx.query_row("SELECT id FROM package_version WHERE ecosystem=?1 AND name=?2 AND version=?3", params![eco, name, version], |r| r.get(0))?;
    tx.execute("DELETE FROM component WHERE package_version_id=?1", params![pv])?;
    tx.execute("DELETE FROM requirement WHERE package_version_id=?1", params![pv])?;
    for c in comps {
        tx.execute(
            "INSERT INTO component (package_version_id, name, version, kind, path, detection, known_vulnerabilities) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![pv, c.name, c.version, c.kind, c.path, c.detection, c.known_vulnerabilities],
        )?;
    }
    for r in reqs {
        tx.execute("INSERT INTO requirement (package_version_id, name, evidence_file, evidence_line) VALUES (?1,?2,?3,?4)", params![pv, r.name, r.evidence_file, r.evidence_line])?;
    }
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    for (t, findings) in per_target {
        let target = format!("php:{t}");
        let errors = findings.iter().filter(|f| f.severity == "error").count() as i64;
        let warnings = findings.len() as i64 - errors;
        let verdict = if errors > 0 {
            "breaks"
        } else if encoded > 0 {
            "unknown"
        } else if warnings > 0 {
            "issues"
        } else {
            "compatible"
        };
        tx.execute("DELETE FROM result WHERE package_version_id=?1 AND target=?2", params![pv, target])?;
        tx.execute(
            "INSERT INTO result (package_version_id, target, run_id, verdict, errors, warnings, scanned_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![pv, target, run_id, verdict, errors, warnings, now],
        )?;
        let rid = tx.last_insert_rowid();
        for f in findings {
            tx.execute(
                "INSERT INTO finding (result_id, rule_id, severity, confidence, message, file, line, component, analyzer) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![rid, f.rule_id, f.severity, f.confidence, f.message, f.file, f.line, f.component, f.analyzer],
            )?;
        }
        trace::log(run_id, "info", "store", "result stored", json!({"target": target, "verdict": verdict, "errors": errors, "warnings": warnings}));
    }
    tx.commit()?;
    Ok(())
}

// ---------- Export ----------

fn export(conn: &Connection, out: &Path) -> Result<usize> {
    let mut n = 0;
    let mut stmt = conn.prepare("SELECT id, ecosystem, name, version FROM package_version ORDER BY ecosystem, name")?;
    let pvs: Vec<(i64, String, String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
    let mut index: std::collections::BTreeMap<(String, String), Vec<String>> = Default::default();
    for (pv, eco, name, version) in pvs {
        let mut targets = serde_json::Map::new();
        let mut scanned_at = String::new();
        let mut rs = conn.prepare("SELECT id, target, verdict, errors, warnings, scanned_at FROM result WHERE package_version_id=?1 ORDER BY target")?;
        let results: Vec<(i64, String, String, i64, i64, String)> =
            rs.query_map(params![pv], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<Result<_, _>>()?;
        if results.is_empty() {
            continue;
        }
        for (rid, target, verdict, errors, warnings, at) in results {
            scanned_at = scanned_at.max(at);
            let mut fs = conn.prepare("SELECT rule_id, severity, confidence, message, file, line, component, analyzer FROM finding WHERE result_id=?1 ORDER BY severity, file, line")?;
            let findings: Vec<serde_json::Value> = fs
                .query_map(params![rid], |r| {
                    Ok(json!({"rule_id": r.get::<_, String>(0)?, "severity": r.get::<_, String>(1)?, "confidence": r.get::<_, String>(2)?,
                        "message": r.get::<_, String>(3)?, "file": r.get::<_, String>(4)?, "line": r.get::<_, i64>(5)?,
                        "component": r.get::<_, Option<String>>(6)?, "analyzer": r.get::<_, String>(7)?}))
                })?
                .collect::<Result<_, _>>()?;
            targets.insert(target, json!({"verdict": verdict, "errors": errors, "warnings": warnings, "findings": findings}));
        }
        let comps: Vec<serde_json::Value> = conn
            .prepare("SELECT name, version, kind, path, detection, known_vulnerabilities FROM component WHERE package_version_id=?1 ORDER BY name")?
            .query_map(params![pv], |r| {
                Ok(json!({"name": r.get::<_, String>(0)?, "version": r.get::<_, Option<String>>(1)?, "kind": r.get::<_, String>(2)?,
                    "path": r.get::<_, String>(3)?, "detection": r.get::<_, String>(4)?, "known_vulnerabilities": r.get::<_, i64>(5)?}))
            })?
            .collect::<Result<_, _>>()?;
        let reqs: Vec<serde_json::Value> = conn
            .prepare("SELECT name, evidence_file, evidence_line FROM requirement WHERE package_version_id=?1 ORDER BY name")?
            .query_map(params![pv], |r| Ok(json!({"name": r.get::<_, String>(0)?, "evidence_file": r.get::<_, String>(1)?, "evidence_line": r.get::<_, i64>(2)?})))?
            .collect::<Result<_, _>>()?;
        let doc = json!({
            "schema": 1, "ecosystem": eco, "name": name, "version": version, "scanned_at": scanned_at,
            "engine_version": trace::ENGINE_VERSION, "targets": targets, "components": comps, "requirements": reqs
        });
        let dir = out.join("v1").join(&eco).join(&name);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(format!("{version}.json")), serde_json::to_vec(&doc)?)?;
        index.entry((eco, name)).or_default().push(version);
        n += 1;
    }
    for ((eco, name), mut versions) in index {
        versions.sort_by(|a, b| detect::ver_cmp(a, b));
        let latest = versions.last().cloned();
        let dir = out.join("v1").join(&eco).join(&name);
        std::fs::write(dir.join("index.json"), serde_json::to_vec(&json!({"schema": 1, "ecosystem": eco, "name": name, "versions": versions, "latest": latest}))?)?;
        n += 1;
    }
    Ok(n)
}

// ---------- Trace viewer & health ----------

fn show_runs(conn: &Connection, id: Option<String>, failed: bool, limit: u32) -> Result<()> {
    if let Some(id) = id {
        let run: Option<serde_json::Value> = conn
            .query_row("SELECT id, kind, input, status, error, started_at, duration_ms, analyzer_version, input_hash, engine_version FROM run WHERE id=?1", params![id], |r| {
                Ok(json!({"id": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "input": r.get::<_, String>(2)?, "status": r.get::<_, String>(3)?,
                    "error": r.get::<_, Option<String>>(4)?, "started_at": r.get::<_, String>(5)?, "duration_ms": r.get::<_, Option<i64>>(6)?,
                    "analyzer_version": r.get::<_, Option<String>>(7)?, "input_hash": r.get::<_, Option<String>>(8)?, "engine_version": r.get::<_, String>(9)?}))
            })
            .optional()?;
        let Some(run) = run else { bail!("run not found") };
        println!("{}", serde_json::to_string_pretty(&run)?);
        let mut st = conn.prepare("SELECT step, status, duration_ms, message FROM run_event WHERE run_id=?1 ORDER BY id")?;
        for row in st.query_map(params![id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<String>>(3)?)))? {
            let (step, status, ms, msg) = row?;
            println!("  {status:<6} {ms:>7} ms  {step:<24} {}", msg.unwrap_or_default());
        }
        return Ok(());
    }
    let sql = format!(
        "SELECT id, kind, status, started_at, duration_ms, substr(input,1,90), substr(COALESCE(error,''),1,80) FROM run {} ORDER BY started_at DESC LIMIT {limit}",
        if failed { "WHERE status='failed'" } else { "" }
    );
    let mut st = conn.prepare(&sql)?;
    for row in st.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<i64>>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?))
    })? {
        let (id, kind, status, at, ms, input, err) = row?;
        println!("{id}  {status:<7} {kind:<13} {at}  {:>7} ms  {input} {err}", ms.unwrap_or(0));
    }
    Ok(())
}

fn health(conn: &Connection, hours: u32) -> Result<()> {
    let since = (chrono::Utc::now() - chrono::Duration::hours(hours as i64)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let (total, failed): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(status='failed'),0) FROM run WHERE started_at >= ?1",
        params![since],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let top_errors: Vec<serde_json::Value> = conn
        .prepare("SELECT substr(error,1,120) e, COUNT(*) c FROM run WHERE status='failed' AND started_at >= ?1 GROUP BY e ORDER BY c DESC LIMIT 10")?
        .query_map(params![since], |r| Ok(json!({"error": r.get::<_, String>(0)?, "count": r.get::<_, i64>(1)?})))?
        .collect::<Result<_, _>>()?;
    let slow_steps: Vec<serde_json::Value> = conn
        .prepare("SELECT CASE WHEN step LIKE 'analyze:%' THEN 'analyze' ELSE step END s, COUNT(*), CAST(AVG(duration_ms) AS INT), MAX(duration_ms) FROM run_event WHERE started_at >= ?1 AND duration_ms > 0 GROUP BY s ORDER BY 3 DESC LIMIT 10")?
        .query_map(params![since], |r| Ok(json!({"step": r.get::<_, String>(0)?, "count": r.get::<_, i64>(1)?, "avg_ms": r.get::<_, i64>(2)?, "max_ms": r.get::<_, i64>(3)?})))?
        .collect::<Result<_, _>>()?;
    let verdicts: Vec<serde_json::Value> = conn
        .prepare("SELECT target, verdict, COUNT(*) FROM result GROUP BY target, verdict ORDER BY target, verdict")?
        .query_map([], |r| Ok(json!({"target": r.get::<_, String>(0)?, "verdict": r.get::<_, String>(1)?, "count": r.get::<_, i64>(2)?})))?
        .collect::<Result<_, _>>()?;
    let rate = if total > 0 { failed as f64 / total as f64 } else { 0.0 };
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "window_hours": hours, "runs": total, "failed": failed, "failure_rate": (rate * 1000.0).round() / 1000.0,
            "top_errors": top_errors, "slowest_steps": slow_steps, "verdicts_all_time": verdicts
        }))?
    );
    Ok(())
}
