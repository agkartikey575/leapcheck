//! Static website generator: one honest, data-rich page per scanned plugin, plus home,
//! methodology, sitemap and robots. Pages only say what the scan data says.

use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

const DEADLINE: &str = "2026-12-31T23:59:59Z";
const MAIN_TARGET: &str = "php:8.4";
const TARGETS: [&str; 3] = ["php:8.3", "php:8.4", "php:8.5"];
const MAX_ROWS: usize = 50;

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// Decode the few HTML entities WordPress.org uses in plugin names.
pub fn decode_entities(s: &str) -> String {
    s.replace("&#8211;", "–").replace("&#8212;", "—").replace("&#8217;", "’").replace("&#039;", "'")
        .replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

#[derive(Deserialize, Clone)]
struct Fix {
    rule_prefix: String,
    title: String,
    plain_explanation: String,
    what_to_do: String,
}

fn fix_for<'a>(fixes: &'a [Fix], rule: &str) -> Option<&'a Fix> {
    fixes.iter().filter(|f| rule.starts_with(&f.rule_prefix)).max_by_key(|f| f.rule_prefix.len())
}

struct Target {
    verdict: String,
    errors: i64,
    warnings: i64,
    findings: Vec<(String, String, String, String, String, i64, Option<String>)>, // rule, severity, confidence, message, file, line, component
}

struct Pkg {
    slug: String,
    display: String,
    installs: Option<i64>,
    version: String,
    scanned_at: String,
    targets: BTreeMap<String, Target>,
    components: Vec<(String, Option<String>, String, String, i64)>, // name, version, path, detection, vulns
    requirements: Vec<(String, String, i64)>,
    history: Vec<(String, String)>, // version, verdict on main target
}

fn verdict_word(v: &str) -> &'static str {
    match v {
        "compatible" => "Compatible",
        "issues" => "Runs, with warnings",
        "breaks" => "Errors found",
        _ => "Not scannable",
    }
}

fn chip(v: &str) -> String {
    let (cls, icon) = match v {
        "compatible" => ("ok", "✅"),
        "issues" => ("warn", "⚠️"),
        "breaks" => ("bad", "❌"),
        _ => ("unk", "❓"),
    };
    format!("<span class=\"chip {cls}\">{icon} {}</span>", verdict_word(v))
}

const CSS: &str = r#"
:root{--bg:#fbfaf7;--fg:#1d1f24;--mut:#5d6370;--card:#fff;--line:#e4e1da;--acc:#2f5bd3;--ok:#1f7a45;--warn:#9a6400;--bad:#b3261e;--unk:#5d6370}
@media (prefers-color-scheme:dark){:root{--bg:#14161a;--fg:#e8e6e1;--mut:#a3a8b3;--card:#1c1f24;--line:#2d3138;--acc:#8aa8ff;--ok:#5fcf8c;--warn:#f0b450;--bad:#ff8a80;--unk:#a3a8b3}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.6 system-ui,-apple-system,Segoe UI,Roboto,sans-serif}
.wrap{max-width:920px;margin:0 auto;padding:24px 16px 64px}a{color:var(--acc)}
header.top{display:flex;justify-content:space-between;align-items:center;gap:12px;flex-wrap:wrap;margin-bottom:24px}
.brand{font-weight:800;font-size:20px;text-decoration:none;color:var(--fg)}.brand span{color:var(--acc)}
h1{font-size:clamp(26px,4vw,36px);line-height:1.2;margin:8px 0}h2{font-size:21px;margin:36px 0 10px}
.lead{font-size:18px;color:var(--mut)}.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:18px;margin:14px 0}
.chips{display:flex;gap:10px;flex-wrap:wrap}.chipbox{display:flex;flex-direction:column;gap:4px}.chipbox b{font-size:13px;color:var(--mut)}
.chip{display:inline-block;padding:6px 10px;border-radius:999px;border:1px solid var(--line);font-weight:600;font-size:14px}
.chip.ok{color:var(--ok)}.chip.warn{color:var(--warn)}.chip.bad{color:var(--bad)}.chip.unk{color:var(--unk)}
table{width:100%;border-collapse:collapse;font-size:14px}th,td{text-align:left;padding:8px 6px;border-bottom:1px solid var(--line);vertical-align:top}
th{color:var(--mut);font-weight:600}.scroll{overflow-x:auto}code{font-size:13px;word-break:break-all}
.small{font-size:13px;color:var(--mut)}.cta{display:inline-block;background:var(--acc);color:#fff;padding:10px 16px;border-radius:8px;text-decoration:none;font-weight:600}
@media (prefers-color-scheme:dark){.cta{color:#14161a}}
.count{font-size:clamp(28px,6vw,44px);font-weight:800;letter-spacing:-.5px}
input[type=search]{width:100%;padding:12px;border-radius:8px;border:1px solid var(--line);background:var(--card);color:var(--fg);font-size:16px}
footer{margin-top:48px;border-top:1px solid var(--line);padding-top:16px;font-size:13px;color:var(--mut)}
details summary{cursor:pointer;font-weight:600}
"#;

fn page(title: &str, desc: &str, canonical: &str, noindex: bool, jsonld: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>{t}</title><meta name=\"description\" content=\"{d}\"><link rel=\"canonical\" href=\"{c}\">{robots}\
<meta property=\"og:title\" content=\"{t}\"><meta property=\"og:description\" content=\"{d}\"><meta property=\"og:type\" content=\"website\">\
<style>{css}</style>{jsonld}</head><body><div class=\"wrap\">\
<header class=\"top\"><a class=\"brand\" href=\"/\">Leap<span>Check</span></a><nav class=\"small\"><a href=\"/plugins/\">Plugins</a> · <a href=\"/about/how-we-scan/\">How we scan</a> · <a href=\"https://github.com/agkartikey575/leapcheck\">Open source</a></nav></header>\
{body}\
<footer>LeapCheck checks public plugin code for PHP version compatibility with open-source static analysis. Results are automated and are not a guarantee. \
LeapCheck is not affiliated with WordPress, the WordPress Foundation or any plugin author; names are trademarks of their owners. \
<a href=\"/about/how-we-scan/\">Methodology and limits</a> · <a href=\"/privacy/\">Privacy</a> · <a href=\"https://github.com/agkartikey575/leapcheck/issues/new?title=Wrong%20result\">Report a wrong result</a></footer>\
</div></body></html>",
        t = esc(title), d = esc(desc), c = esc(canonical), css = CSS,
        robots = if noindex { "<meta name=\"robots\" content=\"noindex,follow\">" } else { "" },
    )
}

/// Build the page model from the exported result files (public/v1/...), so the site can be
/// regenerated from public data alone, e.g. after merging parallel scan shards.
fn load(out: &Path) -> Result<Vec<Pkg>> {
    let mut pkgs = vec![];
    let dir = out.join("v1").join("wordpress-plugin");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(pkgs) };
    for e in rd.flatten() {
        let pdir = e.path();
        let Ok(idx) = std::fs::read_to_string(pdir.join("index.json")) else { continue };
        let idx: serde_json::Value = serde_json::from_str(&idx)?;
        let slug = idx["name"].as_str().unwrap_or_default().to_string();
        let mut versions: Vec<String> = idx["versions"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        versions.sort_by(|a, b| crate::detect::ver_cmp(a, b));
        let mut docs = vec![];
        for v in &versions {
            if let Ok(t) = std::fs::read_to_string(pdir.join(format!("{v}.json"))) {
                docs.push(serde_json::from_str::<serde_json::Value>(&t)?);
            }
        }
        let Some(doc) = docs.last().cloned() else { continue };
        let history = docs.iter().map(|d| (d["version"].as_str().unwrap_or("").to_string(), d["targets"][MAIN_TARGET]["verdict"].as_str().unwrap_or("unknown").to_string())).collect();
        let mut targets = BTreeMap::new();
        if let Some(ts) = doc["targets"].as_object() {
            for (t, v) in ts {
                let findings = v["findings"].as_array().map(|a| a.iter().map(|f| (
                    f["rule_id"].as_str().unwrap_or("").to_string(), f["severity"].as_str().unwrap_or("").to_string(),
                    f["confidence"].as_str().unwrap_or("").to_string(), f["message"].as_str().unwrap_or("").to_string(),
                    f["file"].as_str().unwrap_or("").to_string(), f["line"].as_i64().unwrap_or(0), f["component"].as_str().map(String::from),
                )).collect()).unwrap_or_default();
                targets.insert(t.clone(), Target {
                    verdict: v["verdict"].as_str().unwrap_or("unknown").to_string(),
                    errors: v["errors"].as_i64().unwrap_or(0), warnings: v["warnings"].as_i64().unwrap_or(0), findings,
                });
            }
        }
        let components = doc["components"].as_array().map(|a| a.iter().map(|c| (
            c["name"].as_str().unwrap_or("").to_string(), c["version"].as_str().map(String::from),
            c["path"].as_str().unwrap_or("").to_string(), c["detection"].as_str().unwrap_or("").to_string(), c["known_vulnerabilities"].as_i64().unwrap_or(0),
        )).collect()).unwrap_or_default();
        let requirements = doc["requirements"].as_array().map(|a| a.iter().map(|r| (
            r["name"].as_str().unwrap_or("").to_string(), r["evidence_file"].as_str().unwrap_or("").to_string(), r["evidence_line"].as_i64().unwrap_or(0),
        )).collect()).unwrap_or_default();
        pkgs.push(Pkg {
            display: doc["display_name"].as_str().map(String::from).unwrap_or_else(|| slug.clone()),
            installs: doc["active_installs"].as_i64(),
            version: doc["version"].as_str().unwrap_or("").to_string(),
            scanned_at: doc["scanned_at"].as_str().unwrap_or("").to_string(),
            slug, targets, components, requirements, history,
        });
    }
    Ok(pkgs)
}

fn plugin_page(p: &Pkg, fixes: &[Fix], base: &str) -> (String, bool) {
    let main = p.targets.get(MAIN_TARGET);
    let (verdict, e, w) = main.map(|t| (t.verdict.as_str(), t.errors, t.warnings)).unwrap_or(("unknown", 0, 0));
    let date = p.scanned_at.get(..10).unwrap_or("");
    let name = &p.display;
    let es = if e == 1 { "1 error".to_string() } else { format!("{e} errors") };
    let ws = if w == 1 { "1 warning".to_string() } else { format!("{w} warnings") };
    let title = match verdict {
        "compatible" => format!("Is {name} compatible with PHP 8.4? Yes, v{} | LeapCheck", p.version),
        "issues" => format!("Is {name} compatible with PHP 8.4? Runs, {ws} | LeapCheck"),
        "breaks" => format!("Is {name} compatible with PHP 8.4? {es} found | LeapCheck"),
        _ => format!("{name} PHP 8.4 compatibility: not scannable | LeapCheck"),
    };
    let plain = match verdict {
        "compatible" => "no problems found",
        "issues" => "runs, with deprecation warnings",
        "breaks" => "errors found that can stop it working",
        _ => "could not be fully scanned",
    };
    let desc = format!("{name} {} on PHP 8.4: {plain}. {es}, {ws} from a static scan on {date}. See files, lines and fixes, plus PHP 8.3 and 8.5.", p.version);
    let has_unique = main.map_or(false, |t| !t.findings.is_empty()) || !p.components.is_empty() || !p.requirements.is_empty() || p.history.len() >= 2;
    let index = verdict != "unknown" && (has_unique || p.installs.unwrap_or(0) >= 10_000);
    let canonical = format!("{base}/plugins/{}/", p.slug);

    let mut b = String::new();
    let _ = write!(b, "<p class=\"small\"><a href=\"/\">Home</a> › <a href=\"/plugins/\">Plugins</a> › {}</p>", esc(name));
    let _ = write!(b, "<h1>Is {} compatible with PHP 8.4?</h1>", esc(name));
    let answer = match verdict {
        "compatible" => format!("Version {} of {} showed no PHP 8.4 compatibility problems in our static scan of {date}.", esc(&p.version), esc(name)),
        "unknown" => format!("Version {} of {} contains code we could not scan (for example encoded files), so we can't give a verdict.", esc(&p.version), esc(name)),
        _ => format!("Version {} of {} has {es} and {ws} on PHP 8.4 in our static scan of {date}.", esc(&p.version), esc(name)),
    };
    let _ = write!(b, "<p class=\"lead\">{answer}</p><div class=\"card\"><div class=\"chips\">");
    for t in TARGETS {
        let v = p.targets.get(t).map(|x| x.verdict.as_str()).unwrap_or("unknown");
        let _ = write!(b, "<div class=\"chipbox\"><b>PHP {}</b>{}</div>", &t[4..], chip(v));
    }
    let _ = write!(b, "</div><p class=\"small\">Version scanned: {} · Scanned: {date} · <a href=\"/v1/wordpress-plugin/{}/{}.json\">Raw data (JSON)</a> · This is an automated static scan, not a guarantee. <a href=\"/about/how-we-scan/\">How we scan</a></p></div>",
        esc(&p.version), esc(&p.slug), esc(&p.version));

    // What this means
    let _ = write!(b, "<h2>What this means for you</h2><p>{}</p>", match verdict {
        "compatible" => "We found no code in this version that is known to break or be deprecated on PHP 8.4. Still test your whole site on a staging copy before upgrading.",
        "issues" => "The plugin should keep running on PHP 8.4, but it uses features PHP has deprecated. Deprecations don't stop your site today; they can become errors in a later PHP version, and they may fill your error log.",
        "breaks" => "This version uses code that newer PHP versions removed or changed. Depending on which feature is used, a page or the whole site can stop with an error after upgrading. Check the findings below, especially any inside bundled libraries.",
        _ => "Part of this plugin's code is encoded or otherwise unreadable to our scanner, so we can't tell. Ask the plugin's author whether it supports PHP 8.4.",
    });

    // Findings grouped by rule
    if let Some(t) = main {
        if !t.findings.is_empty() {
            let mut groups: BTreeMap<&str, Vec<&(String, String, String, String, String, i64, Option<String>)>> = BTreeMap::new();
            for f in &t.findings {
                groups.entry(f.0.as_str()).or_default().push(f);
            }
            let mut gs: Vec<_> = groups.into_iter().collect();
            gs.sort_by_key(|(_, v)| (match v[0].1.as_str() { "error" => 0, "warning" => 1, _ => 2 }, std::cmp::Reverse(v.len())));
            let _ = write!(b, "<h2>What we found (PHP 8.4)</h2><div class=\"scroll\"><table><tr><th>Severity</th><th>Problem</th><th>Where</th></tr>");
            for (rule, items) in gs.iter().take(MAX_ROWS) {
                let f = items[0];
                let fx = fix_for(fixes, rule);
                let title = fx.map(|x| x.title.clone()).unwrap_or_else(|| f.3.clone());
                let comp = f.6.as_ref().map(|c| format!(" <span class=\"small\">(inside bundled library <b>{}</b>)</span>", esc(c))).unwrap_or_default();
                let more = if items.len() > 1 { format!(" <span class=\"small\">and {} more places</span>", items.len() - 1) } else { String::new() };
                let _ = write!(b, "<tr><td>{}</td><td><b>{}</b><details><summary class=\"small\">Details and fix</summary><p class=\"small\">{}</p>{}<p class=\"small\"><code>{}</code></p></details></td><td><code>{}:{}</code>{}{}</td></tr>",
                    match f.1.as_str() { "error" => "❌ Error", "warning" => "⚠️ Warning", _ => "ℹ️ Guarded" }, esc(&title),
                    esc(&f.3),
                    fx.map(|x| format!("<p class=\"small\">{} <b>What to do:</b> {}</p>", esc(&x.plain_explanation), esc(&x.what_to_do))).unwrap_or_default(),
                    esc(rule), esc(&f.4), f.5, comp, more);
            }
            let _ = write!(b, "</table></div>");
            if gs.len() > MAX_ROWS {
                let _ = write!(b, "<p class=\"small\">Showing {MAX_ROWS} of {} problem types. The full list is in the raw data.</p>", gs.len());
            }
        }
    }
    if !p.components.is_empty() {
        let _ = write!(b, "<h2>Hidden libraries bundled inside</h2><p>Plugins often ship their own copies of other libraries. Old copies are a common reason sites break during PHP upgrades.</p><div class=\"scroll\"><table><tr><th>Library</th><th>Version</th><th>Location</th><th>Known vulnerabilities</th></tr>");
        for (n, v, path, det, vul) in &p.components {
            let _ = write!(b, "<tr><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>", esc(n), esc(v.as_deref().unwrap_or("unknown")), esc(path),
                if *vul > 0 { format!("{vul} (per <a href=\"https://github.com/RetireJS/retire.js\">RetireJS</a> data)") } else if det == "retirejs" { "None known".into() } else { "—".into() });
        }
        let _ = write!(b, "</table></div>");
    }
    if !p.requirements.is_empty() {
        let _ = write!(b, "<h2>Server requirements detected</h2><ul>");
        for (n, f, l) in &p.requirements {
            let _ = write!(b, "<li>Uses the <b>{}</b> PHP extension (<code>{}:{}</code>)</li>", esc(n), esc(f), l);
        }
        let _ = write!(b, "</ul><p class=\"small\">Make sure your host has these enabled on the new PHP version.</p>");
    }
    if p.history.len() >= 2 {
        let _ = write!(b, "<h2>Version history (PHP 8.4)</h2><ul>");
        for (v, verdict) in p.history.iter().rev() {
            let _ = write!(b, "<li>Version {} {}</li>", esc(v), chip(verdict));
        }
        let _ = write!(b, "</ul>");
    }
    let _ = write!(b, "<h2>What to do next</h2><ol><li>Update {} to its newest version, then check again.</li><li>Test the upgrade on a staging copy of your site first.</li><li>If problems remain, ask the author in the plugin's <a href=\"https://wordpress.org/support/plugin/{}/\">support forum</a>.</li></ol>\
<div class=\"card\"><b>Check your whole site in seconds.</b> The free LeapCheck plugin checks every plugin and theme you have installed against PHP 8.3, 8.4 and 8.5. <p><a class=\"cta\" href=\"https://github.com/agkartikey575/leapcheck\">Get LeapCheck</a></p></div>",
        esc(name), esc(&p.slug));
    let _ = write!(b, "<p class=\"small\">Not affiliated with the author of {}. Results are generated automatically from the public code of version {}. Last updated {date}.</p>", esc(name), esc(&p.version));

    let jsonld = format!(
        "<script type=\"application/ld+json\">{}</script>",
        serde_json::json!({
            "@context": "https://schema.org",
            "@graph": [
                {"@type": "WebPage", "name": title, "url": canonical, "dateModified": p.scanned_at,
                 "about": {"@type": "SoftwareApplication", "name": name, "softwareVersion": p.version, "applicationCategory": "WebApplication",
                           "url": format!("https://wordpress.org/plugins/{}/", p.slug)}},
                {"@type": "BreadcrumbList", "itemListElement": [
                    {"@type": "ListItem", "position": 1, "name": "Home", "item": format!("{base}/")},
                    {"@type": "ListItem", "position": 2, "name": "Plugins", "item": format!("{base}/plugins/")},
                    {"@type": "ListItem", "position": 3, "name": name, "item": canonical}]}
            ]
        }).to_string().replace("</", "<\\/")
    );
    (page(&title, &desc, &canonical, !index, &jsonld, &b), index)
}

fn write(out: &Path, rel: &str, content: &str) -> Result<()> {
    let p = out.join(rel);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, content)?;
    Ok(())
}

pub fn generate(rules: &Path, out: &Path, base: &str) -> Result<usize> {
    let base = base.trim_end_matches('/');
    let fixes: Vec<Fix> = std::fs::read_to_string(rules.join("fix-dictionary.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let mut pkgs = load(out)?;
    pkgs.sort_by(|a, b| b.installs.unwrap_or(0).cmp(&a.installs.unwrap_or(0)).then(a.slug.cmp(&b.slug)));
    let mut n = 0;
    let mut sitemap = vec![(format!("{base}/"), String::new()), (format!("{base}/plugins/"), String::new()), (format!("{base}/about/how-we-scan/"), String::new())];
    let mut rows = String::new();
    for p in &pkgs {
        let (html, index) = plugin_page(p, &fixes, base);
        write(out, &format!("plugins/{}/index.html", p.slug), &html)?;
        n += 1;
        if index {
            sitemap.push((format!("{base}/plugins/{}/", p.slug), p.scanned_at.get(..10).unwrap_or("").to_string()));
        }
        let v = p.targets.get(MAIN_TARGET).map(|t| t.verdict.as_str()).unwrap_or("unknown");
        let _ = write!(rows, "<tr><td><a href=\"/plugins/{}/\">{}</a></td><td>{}</td><td>{}</td></tr>", esc(&p.slug), esc(&p.display), esc(&p.version), chip(v));
    }
    // Plugin directory page
    let list = format!("<h1>Plugin PHP compatibility results</h1><p class=\"lead\">{} plugins checked against PHP 8.3, 8.4 and 8.5. More are added every night.</p><div class=\"scroll\"><table><tr><th>Plugin</th><th>Version</th><th>PHP 8.4</th></tr>{rows}</table></div>", pkgs.len());
    write(out, "plugins/index.html", &page("WordPress plugin PHP 8.4 compatibility results | LeapCheck", "Which WordPress plugins work on PHP 8.3, 8.4 and 8.5? Automated static scans of public plugin code, with files, lines and fixes.", &format!("{base}/plugins/"), false, "", &list))?;
    n += 1;

    // Home
    let counts = pkgs.iter().fold([0usize; 4], |mut c, p| {
        match p.targets.get(MAIN_TARGET).map(|t| t.verdict.as_str()).unwrap_or("unknown") {
            "compatible" => c[0] += 1,
            "issues" => c[1] += 1,
            "breaks" => c[2] += 1,
            _ => c[3] += 1,
        }
        c
    });
    let home = format!(r#"<h1>Check before you leap.</h1>
<p class="lead">PHP 8.2 stops getting security fixes on 31 December 2026, and hosts are moving sites to newer PHP. LeapCheck tells you which of your plugins will break on PHP 8.3, 8.4 and 8.5, before you upgrade, including old libraries hidden inside them.</p>
<div class="card"><div class="small">PHP 8.2 security support ends in</div><div class="count" id="cd">…</div></div>
<div class="card"><label for="q"><b>Is your plugin ready?</b> Type its WordPress.org slug (the folder name, e.g. <code>contact-form-7</code>)</label><p><input type="search" id="q" placeholder="plugin-slug" autocomplete="off"></p><div id="res" class="small"></div></div>
<h2>What we've checked so far</h2><p>{total} plugins scanned for PHP 8.4: <b>{c0}</b> compatible, <b>{c1}</b> run with warnings, <b>{c2}</b> have errors, <b>{c3}</b> couldn't be scanned. <a href="/plugins/">See all results</a></p>
<div class="card"><b>Check your whole site at once.</b> The free LeapCheck plugin checks every plugin and theme you have installed. It only looks up public plugin names and versions, only after you agree, and sends nothing about your site.<p><a class="cta" href="https://github.com/agkartikey575/leapcheck">Get LeapCheck</a></p></div>
<script>
(function(){{var d=new Date("{DEADLINE}"),el=document.getElementById("cd");function t(){{var s=Math.max(0,(d-new Date())/1000|0);el.textContent=s?Math.floor(s/86400)+" days "+Math.floor(s%86400/3600)+" hours":"Support has ended";}}t();setInterval(t,60000);
var q=document.getElementById("q"),r=document.getElementById("res"),tm;q.addEventListener("input",function(){{clearTimeout(tm);tm=setTimeout(go,300);}});
function go(){{var s=q.value.trim().toLowerCase();if(!/^[a-z0-9][a-z0-9-]{{0,199}}$/.test(s)){{r.textContent="";return;}}
fetch("/v1/wordpress-plugin/"+s+"/index.json").then(function(x){{if(!x.ok)throw 0;return x.json();}}).then(function(i){{r.innerHTML='Found: <a href="/plugins/'+s+'/">'+s+' (version '+i.latest+')</a>';}}).catch(function(){{r.textContent="Not in our database yet. Popular plugins are added first; check back soon.";}});}}}})();
</script>"#,
        total = pkgs.len(), c0 = counts[0], c1 = counts[1], c2 = counts[2], c3 = counts[3]);
    write(out, "index.html", &page("Will your WordPress site break on PHP 8.4? | LeapCheck", "Check which plugins break on PHP 8.3, 8.4 and 8.5 before PHP 8.2 support ends on 31 Dec 2026. Free, automated, open source.", &format!("{base}/"), false, "", &home))?;
    n += 1;

    let about = r#"<h1>How we scan</h1>
<p class="lead">LeapCheck reads the public code of each plugin version and checks it against the known changes in every PHP version. It never runs plugin code.</p>
<h2>What we check</h2><ul>
<li><b>PHP compatibility</b> with <a href="https://github.com/PHPCompatibility/PHPCompatibility">PHPCompatibility</a>, an open-source rule set built from PHP's official change notes, for PHP 8.3, 8.4 and 8.5.</li>
<li><b>Hidden libraries</b>: copies of other libraries bundled inside a plugin, found by package manifests, file signatures and <a href="https://github.com/RetireJS/retire.js">RetireJS</a> open data (Apache-2.0) for JavaScript.</li>
<li><b>Server requirements</b>: PHP extensions the code appears to use.</li></ul>
<h2>Verdicts</h2><ul><li>✅ <b>Compatible</b>: nothing known to break or be deprecated.</li><li>⚠️ <b>Runs, with warnings</b>: deprecated features only.</li><li>❌ <b>Errors found</b>: removed or changed features that can stop code working.</li><li>❓ <b>Not scannable</b>: encoded or unreadable code.</li></ul>
<h2>Limits</h2><p>Static analysis can't see code that is generated while running, <code>eval</code>, or encoded files, and it can't find a plugin's own logic bugs. A clean result is not a guarantee: always test upgrades on a staging copy first.</p>
<h2>Freshness and corrections</h2><p>New plugin versions are re-scanned automatically. Every result records the engine and rule versions that produced it. If a result looks wrong, <a href="https://github.com/agkartikey575/leapcheck/issues/new?title=Wrong%20result">report it</a>; plugin authors can request a re-scan the same way.</p>
<h2>Open source</h2><p>The scanner is open source: <a href="https://github.com/agkartikey575/leapcheck">github.com/agkartikey575/leapcheck</a>.</p>"#;
    write(out, "about/how-we-scan/index.html", &page("How LeapCheck scans plugins for PHP compatibility", "Methodology, verdicts and limits of LeapCheck's automated PHP compatibility scans.", &format!("{base}/about/how-we-scan/"), false, "", about))?;
    n += 1;

    let privacy = r#"<h1>Privacy</h1><p class="small">Last updated: 16 November 2026</p>
<h2>The short version</h2><ul>
<li><b>We don't collect your site's content, your users' data, or your name.</b></li>
<li>The LeapCheck plugin does nothing online until you click its consent button.</li>
<li>After that, it downloads public result files from this website, one per plugin version. Like any website visit, our hosting provider briefly sees your server's IP address. We don't keep a database of who asked for what.</li>
<li>This website uses <b>no cookies, no analytics and no advertising trackers</b>.</li></ul>
<h2>Who we are</h2><p>LeapCheck is an open-source project maintained on GitHub at <a href="https://github.com/agkartikey575/leapcheck">github.com/agkartikey575/leapcheck</a>. For privacy questions, contact the maintainer privately through GitHub.</p>
<h2>The WordPress plugin</h2><p>To build your report, the plugin reads, on your own server, only the names (slugs) and version numbers of your installed plugins and themes, and your PHP version. Nothing else: no posts, pages, users, orders, emails or settings.</p>
<p>After you opt in, it requests public files such as <code>/v1/wordpress-plugin/contact-form-7/5.1.json</code>. Each request contains only the slug and version in the address, your server's IP address (which every internet request carries) and a generic label <code>LeapCheck/version</code>. The plugin deliberately replaces WordPress's default request label, which would contain your site's address. We do not receive or store the list of plugins you have.</p>
<p>You can turn checking off at any time on the plugin's page (Tools → LeapCheck), which also deletes cached results. Deleting the plugin removes all its settings. If a plugin is not in our database, the plugin says so; it never uploads that plugin's code.</p>
<h2>This website</h2><p>No cookies, no analytics, no advertising or social-media trackers, no third-party fonts or scripts. Our hosting provider (Vercel Inc.) receives standard technical data with each request (IP address, browser type, page and time) to deliver pages and protect against attacks, and keeps short-lived request logs. We don't export or combine them.</p>
<h2>Legal basis and your rights</h2><p>We process IP addresses and request details under our legitimate interest in delivering the files and pages you ask for and keeping the service secure (Article 6(1)(f) GDPR). You can ask for access, correction, deletion, restriction or objection; because we keep no database of plugin requests, we usually hold nothing that can be linked to you. You can also complain to your data-protection authority.</p>
<h2>Changes</h2><p>We will update this page before we change what data we process, including before any accounts or paid plans launch. The plugin will ask you again before any new data leaves your site.</p>"#;
    write(out, "privacy/index.html", &page("Privacy | LeapCheck", "LeapCheck collects no site data: no cookies, no analytics, and the plugin only fetches public files after you opt in.", &format!("{base}/privacy/"), false, "", privacy))?;
    n += 1;
    sitemap.push((format!("{base}/privacy/"), String::new()));

    let mut sm = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    for (u, lm) in &sitemap {
        if lm.is_empty() {
            let _ = writeln!(sm, "<url><loc>{}</loc></url>", esc(u));
        } else {
            let _ = writeln!(sm, "<url><loc>{}</loc><lastmod>{lm}</lastmod></url>", esc(u));
        }
    }
    sm.push_str("</urlset>\n");
    write(out, "sitemap.xml", &sm)?;
    write(out, "robots.txt", &format!("User-agent: *\nAllow: /\nSitemap: {base}/sitemap.xml\n"))?;
    write(out, "vercel.json", r#"{
  "cleanUrls": true,
  "trailingSlash": true,
  "headers": [
    {"source": "/v1/(.*)", "headers": [
      {"key": "Cache-Control", "value": "public, max-age=3600"},
      {"key": "Access-Control-Allow-Origin", "value": "*"},
      {"key": "X-Content-Type-Options", "value": "nosniff"}]},
    {"source": "/(.*)", "headers": [
      {"key": "X-Content-Type-Options", "value": "nosniff"},
      {"key": "Referrer-Policy", "value": "strict-origin-when-cross-origin"}]}
  ]
}
"#)?;
    n += 4;
    Ok(n)
}
