//! PHP analyzer: runs PHP_CodeSniffer + PHPCompatibility as a separate tool and maps its JSON report.

use crate::model::{Analyzer, Finding};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct PhpCompatibility {
    pub phpcs: PathBuf,
}

impl PhpCompatibility {
    pub fn locate(tools_dir: &Path) -> Result<Self> {
        let phpcs = tools_dir.join("vendor/bin/phpcs");
        if !phpcs.exists() {
            bail!("phpcs not found at {} (run composer install in tools/php)", phpcs.display());
        }
        Ok(Self { phpcs })
    }
}

impl Analyzer for PhpCompatibility {
    fn id(&self) -> &'static str {
        "php:phpcompatibility"
    }

    fn version(&self) -> String {
        Command::new(&self.phpcs)
            .arg("--version")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "unknown".into())
    }

    fn analyze(&self, root: &Path, target: &str) -> Result<Vec<Finding>> {
        // Only files that can run on a live site are scanned (see `runtime_files`). The list goes to
        // phpcs through a file, so very large packages never hit the command-line length limit.
        let files = runtime_files(root);
        if files.is_empty() {
            return Ok(vec![]);
        }
        let list = std::env::temp_dir().join(format!(
            "leapcheck-files-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        let body: String = files.iter().map(|f| format!("{}\n", f.display())).collect();
        std::fs::write(&list, body).context("writing phpcs file list")?;
        let out = Command::new(&self.phpcs)
            .args([
                "--standard=PHPCompatibility",
                "--runtime-set",
                "testVersion",
                target,
                "--extensions=php,inc",
                "--report=json",
                "-q",
                "--parallel=4",
            ])
            .arg(format!("--file-list={}", list.display()))
            .output();
        let _ = std::fs::remove_file(&list);
        let out = out.context("running phpcs")?;
        // PHPCS exit codes are bit flags (1 = issues, 2 = fixable issues, higher bits = processing
        // problems). Trust the JSON report: if it parsed and has totals, the scan ran.
        let code = out.status.code().unwrap_or(-1);
        let report: serde_json::Value = match serde_json::from_slice::<serde_json::Value>(&out.stdout) {
            Ok(v) if v.get("totals").is_some() => v,
            _ => bail!(
                "phpcs failed (exit {code}): {} {}",
                String::from_utf8_lossy(&out.stderr).chars().take(300).collect::<String>(),
                String::from_utf8_lossy(&out.stdout).chars().take(300).collect::<String>()
            ),
        };
        Ok(parse_report(&report, root, self.id()))
    }
}

/// Map a phpcs JSON report to findings. `Internal.*` messages (no PHP code found in a file,
/// mixed line endings) are tool notes about the file itself, never a compatibility change.
pub fn parse_report(report: &serde_json::Value, root: &Path, analyzer: &str) -> Vec<Finding> {
    let mut findings = vec![];
    if let Some(files) = report.get("files").and_then(|f| f.as_object()) {
        for (file, v) in files {
            let rel = Path::new(file).strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| file.clone());
            for m in v.get("messages").and_then(|m| m.as_array()).into_iter().flatten() {
                let rule_id = m.get("source").and_then(|s| s.as_str()).unwrap_or("").to_string();
                if rule_id.starts_with("Internal.") {
                    continue;
                }
                let sev = m.get("type").and_then(|t| t.as_str()).unwrap_or("WARNING").to_lowercase();
                findings.push(Finding {
                    rule_id,
                    confidence: if sev == "error" { "high".into() } else { "medium".into() },
                    severity: sev,
                    message: m.get("message").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                    file: rel.clone(),
                    line: m.get("line").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
                    component: None,
                    analyzer: analyzer.into(),
                });
            }
        }
    }
    findings
}

/// Directory names whose PHP code is a bundled library's demo, example or command-line tool.
const VENDOR_NON_RUNTIME_DIRS: &[&str] = &["gui", "examples", "example", "demo", "demos", "bin"];

/// Why a file (path relative to the package root, `/` separators) never runs on a live site,
/// or None if it may run. `first_bytes` is the start of the file.
pub fn non_runtime_reason(rel: &str, first_bytes: &[u8]) -> Option<&'static str> {
    let segs: Vec<&str> = rel.split('/').collect();
    let (dirs, fname) = segs.split_at(segs.len().saturating_sub(1));
    let fname = fname.first().copied().unwrap_or("");
    if fname.ends_with(".min.php") {
        return Some("encoded or minified file");
    }
    if dirs.iter().any(|d| *d == "node_modules") {
        return Some("node_modules");
    }
    // Unit-test suites (PHPUnit and similar) are never loaded by WordPress.
    if dirs.iter().any(|d| matches!(*d, "test" | "tests" | "Test" | "Tests")) {
        return Some("test suite");
    }
    // Inside a bundled library: demo GUIs, examples, CLI tools and code-generator templates.
    if let Some(v) = dirs.iter().position(|d| d.starts_with("vendor")) {
        let inner = &dirs[v + 1..];
        if inner.iter().any(|d| VENDOR_NON_RUNTIME_DIRS.contains(d)) {
            return Some("bundled library demo, example or command-line tool");
        }
        if fname.ends_with(".tpl.php") && inner.iter().any(|d| *d == "resources") {
            return Some("bundled code-generator template");
        }
    }
    // Command-line scripts (`#!/usr/bin/env php`) are not loaded by a web request.
    if first_bytes.starts_with(b"#!") {
        return Some("command-line script");
    }
    None
}

/// PHP files (.php, .inc) under `root` that can run on a live site, sorted.
pub fn runtime_files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in walkdir::WalkDir::new(root).sort_by_file_name().into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        let p = e.path();
        if !p.extension().map_or(false, |x| x == "php" || x == "inc") {
            continue;
        }
        let rel = p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/");
        let mut head = [0u8; 2];
        let n = std::fs::File::open(p).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
        if non_runtime_reason(&rel, &head[..n]).is_none() {
            out.push(p.to_path_buf());
        }
    }
    out
}
