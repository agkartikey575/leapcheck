//! Hidden-library and requirement detection. Entirely data-driven: rules live in engine/rules/*.json.

use crate::model::{Component, Requirement};
use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use walkdir::WalkDir;

const MAX_READ: usize = 64 * 1024;

#[derive(Deserialize)]
struct SignatureRule {
    name: String,
    match_file_regex: String,
    version_regex: String,
}

#[derive(Deserialize)]
struct ExtensionRule {
    extension: String,
    patterns: Vec<String>,
}

pub struct Rules {
    signatures: Vec<(String, Regex, Regex)>,
    extensions: Vec<(String, Vec<Regex>)>,
    retire: Option<serde_json::Value>,
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&s).with_context(|| format!("parsing {}", path.display()))?))
}

impl Rules {
    pub fn load(dir: &Path) -> Result<Self> {
        let sigs: Vec<SignatureRule> = read_json(&dir.join("php-signatures.json"))?.unwrap_or_default();
        let exts: Vec<ExtensionRule> = read_json(&dir.join("php-extensions.json"))?.unwrap_or_default();
        let retire: Option<serde_json::Value> = read_json(&dir.join("retire-jsrepository.json"))?;
        let mut signatures = vec![];
        for s in sigs {
            match (Regex::new(&s.match_file_regex), Regex::new(&s.version_regex)) {
                (Ok(f), Ok(v)) => signatures.push((s.name, f, v)),
                _ => eprintln!("{{\"level\":\"warn\",\"msg\":\"invalid signature rule\",\"rule\":\"{}\"}}", s.name),
            }
        }
        let extensions = exts
            .into_iter()
            .map(|e| (e.extension, e.patterns.iter().filter_map(|p| Regex::new(p).ok()).collect()))
            .collect();
        Ok(Rules { signatures, extensions, retire })
    }

    pub fn summary(&self) -> String {
        format!("signatures={} extensions={} retirejs={}", self.signatures.len(), self.extensions.len(), self.retire.is_some())
    }
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn head(p: &Path) -> Option<String> {
    let bytes = std::fs::read(p).ok()?;
    let n = bytes.len().min(MAX_READ);
    Some(String::from_utf8_lossy(&bytes[..n]).into_owned())
}

/// Find libraries bundled inside `root`.
pub fn components(root: &Path, rules: &Rules) -> Vec<Component> {
    let mut out: BTreeMap<(String, String), Component> = BTreeMap::new();
    let mut add = |c: Component| {
        out.entry((c.name.clone(), c.path.clone())).or_insert(c);
    };

    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        let p = entry.path();
        let r = rel(root, p);
        let fname = p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();

        // 1. Composer packages vendored inside the plugin.
        if r.ends_with("vendor/composer/installed.json") {
            if let Some(v) = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
                let pkgs = v.get("packages").cloned().unwrap_or(v.clone());
                if let Some(arr) = pkgs.as_array() {
                    let base = r.trim_end_matches("composer/installed.json").to_string();
                    for pkg in arr {
                        if let Some(name) = pkg.get("name").and_then(|n| n.as_str()) {
                            add(Component {
                                name: name.to_string(),
                                version: pkg.get("version").and_then(|v| v.as_str()).map(|s| s.trim_start_matches('v').to_string()),
                                kind: "php".into(),
                                path: format!("{base}{name}"),
                                detection: "composer-installed".into(),
                                known_vulnerabilities: 0,
                            });
                        }
                    }
                }
            }
            continue;
        }

        // 2. npm packages shipped inside the plugin (node_modules or bundled package.json).
        if fname == "package.json" && r.contains("node_modules/") {
            if let Some(v) = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
                if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
                    add(Component {
                        name: name.to_string(),
                        version: v.get("version").and_then(|x| x.as_str()).map(String::from),
                        kind: "npm".into(),
                        path: r.trim_end_matches("/package.json").to_string(),
                        detection: "package-json".into(),
                        known_vulnerabilities: 0,
                    });
                }
            }
            continue;
        }

        // 3. Known PHP libraries by signature.
        if fname.ends_with(".php") || fname.ends_with(".inc") {
            for (name, file_re, ver_re) in &rules.signatures {
                if file_re.is_match(&r) {
                    let version = head(p).and_then(|h| ver_re.captures(&h).and_then(|c| c.get(1)).map(|m| m.as_str().to_string()));
                    let dir = r.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                    add(Component { name: name.clone(), version, kind: "php".into(), path: dir, detection: "signature".into(), known_vulnerabilities: 0 });
                }
            }
        }

        // 4. JavaScript libraries (often minified) via RetireJS open data.
        if fname.ends_with(".js") {
            if let Some(c) = retire_detect(rules, &r, &fname, p) {
                add(c);
            }
        }
    }
    out.into_values().collect()
}

/// RetireJS extractor patterns use §§version§§ as the version placeholder.
fn retire_pattern(p: &str) -> Option<Regex> {
    let p = p.replace("§§version§§", r"[0-9][0-9a-zA-Z._\-]*");
    // Make the version group capturing: the replaced text is wrapped below.
    let p = p.replacen(r"[0-9][0-9a-zA-Z._\-]*", r"([0-9][0-9a-zA-Z._\-]*)", 1);
    Regex::new(&p).ok()
}

fn retire_detect(rules: &Rules, rel_path: &str, fname: &str, p: &Path) -> Option<Component> {
    let repo = rules.retire.as_ref()?.as_object()?;
    let mut content: Option<String> = None;
    for (lib, def) in repo {
        if lib == "retire-example" {
            continue;
        }
        let ex = def.get("extractors")?;
        let mut version: Option<String> = None;
        if let Some(arr) = ex.get("filename").and_then(|a| a.as_array()) {
            for pat in arr.iter().filter_map(|x| x.as_str()) {
                if let Some(re) = retire_pattern(pat) {
                    if let Some(c) = re.captures(fname) {
                        version = c.get(1).map(|m| m.as_str().to_string());
                        break;
                    }
                }
            }
        }
        if version.is_none() {
            if let Some(arr) = ex.get("filecontent").and_then(|a| a.as_array()) {
                if content.is_none() {
                    content = head(p);
                }
                let text = content.as_deref().unwrap_or("");
                for pat in arr.iter().filter_map(|x| x.as_str()) {
                    if let Some(re) = retire_pattern(pat) {
                        if let Some(c) = re.captures(text) {
                            version = c.get(1).map(|m| m.as_str().to_string());
                            break;
                        }
                    }
                }
            }
        }
        if let Some(v) = version {
            let vulns = def
                .get("vulnerabilities")
                .and_then(|a| a.as_array())
                .map(|arr| arr.iter().filter(|vu| vuln_applies(vu, &v)).count() as u32)
                .unwrap_or(0);
            return Some(Component {
                name: lib.clone(),
                version: Some(v),
                kind: "js".into(),
                path: rel_path.to_string(),
                detection: "retirejs".into(),
                known_vulnerabilities: vulns,
            });
        }
    }
    None
}

fn parse_ver(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).take(4).map(|s| s.parse().unwrap_or(0)).collect()
}

pub fn ver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (x, y) = (parse_ver(a), parse_ver(b));
    for i in 0..x.len().max(y.len()) {
        let o = x.get(i).unwrap_or(&0).cmp(y.get(i).unwrap_or(&0));
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    std::cmp::Ordering::Equal
}

fn vuln_applies(v: &serde_json::Value, ver: &str) -> bool {
    let below = v.get("below").and_then(|x| x.as_str());
    let at_or_above = v.get("atOrAbove").and_then(|x| x.as_str());
    match below {
        Some(b) => ver_cmp(ver, b).is_lt() && at_or_above.map_or(true, |a| !ver_cmp(ver, a).is_lt()),
        None => false,
    }
}

/// Server extensions the PHP code appears to need.
pub fn requirements(root: &Path, rules: &Rules) -> Vec<Requirement> {
    let mut found: BTreeMap<String, Requirement> = BTreeMap::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        let p = entry.path();
        if p.extension().map_or(true, |e| e != "php") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(p) else { continue };
        for (ext, pats) in &rules.extensions {
            if found.contains_key(ext) {
                continue;
            }
            'outer: for (i, line) in text.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("//") || t.starts_with('*') || t.starts_with('#') {
                    continue;
                }
                for re in pats {
                    if re.is_match(line) {
                        found.insert(ext.clone(), Requirement { name: ext.clone(), evidence_file: rel(root, p), evidence_line: (i + 1) as u32 });
                        break 'outer;
                    }
                }
            }
        }
    }
    found.into_values().collect()
}

/// Attribute a finding's file to the deepest component whose path contains it.
pub fn attribute(file: &str, components: &[Component]) -> Option<String> {
    components
        .iter()
        .filter(|c| !c.path.is_empty() && (file == c.path || file.starts_with(&format!("{}/", c.path))))
        .max_by_key(|c| c.path.len())
        .map(|c| match &c.version {
            Some(v) => format!("{}@{}", c.name, v),
            None => c.name.clone(),
        })
}

/// Extract the PHP identifier a PHPCompatibility rule refers to, e.g.
/// "PHPCompatibility.FunctionUse.RemovedFunctions.mysql_queryDeprecatedRemoved" -> "mysql_query".
fn rule_ident(rule: &str) -> Option<String> {
    let last = rule.rsplit('.').next()?;
    let mut s = last.to_string();
    for suf in ["DeprecatedRemoved", "Removed", "Deprecated", "Found", "Changed", "Numeric"] {
        if let Some(x) = s.strip_suffix(suf) {
            s = x.to_string();
            break;
        }
    }
    if s.is_empty() || s.chars().next()?.is_uppercase() {
        return None; // generic sniffs (e.g. "Removed" syntax rules) have no identifier
    }
    Some(s.to_lowercase())
}

/// Downgrade errors in code that only runs when the old feature exists, e.g.
/// `if (function_exists('mcrypt_encrypt')) { ... }`, `extension_loaded('mysql')`, `defined('X')`,
/// or an explicit PHP version check. Such code is skipped on new PHP, so it can't break there.
/// Guarded findings become severity "info" (shown, but not counted in the verdict).
pub fn apply_guards(root: &Path, findings: &mut [crate::model::Finding]) {
    let version_guard = Regex::new(r"(?i)(PHP_VERSION_ID\s*[<>]=?|version_compare\s*\(\s*(PHP_VERSION|phpversion\s*\(\s*\)))").unwrap();
    let mut cache: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in findings.iter_mut().filter(|f| f.severity == "error") {
        let lines = cache.entry(f.file.clone()).or_insert_with(|| {
            std::fs::read_to_string(root.join(&f.file)).map(|t| t.lines().map(|l| l.to_lowercase()).collect()).unwrap_or_default()
        });
        if lines.is_empty() || f.line == 0 {
            continue;
        }
        let end = (f.line as usize).min(lines.len());
        let start = end.saturating_sub(80);
        let window = &lines[start..end];
        let ident = rule_ident(&f.rule_id);
        // Extension rules (e.g. "mcrypt") are guarded by checks for any function of that extension.
        let guarded_by_ident = ident.as_ref().map_or(false, |id| {
            let needle = regex::escape(id);
            let re = Regex::new(&format!(
                r#"(function_exists|extension_loaded|defined|is_callable|class_exists)\s*\(\s*['"]\\?{needle}"#
            ))
            .unwrap();
            window.iter().any(|l| re.is_match(l))
        });
        let near_start = end.saturating_sub(15);
        let guarded_by_version = lines[near_start..end].iter().any(|l| version_guard.is_match(l));
        if guarded_by_ident || guarded_by_version {
            f.severity = "info".into();
            f.confidence = "low".into();
            f.message = format!("Guarded (only runs when the old feature is available, so it is skipped on newer PHP): {}", f.message);
        }
    }
}

/// Old extensions removed from PHP long ago, and the modern alternatives code switches to.
const FALLBACKS: &[(&str, &[&str])] = &[
    ("mysql", &["mysqli"]),            // removed in PHP 7.0
    ("mcrypt", &["openssl_", "sodium_"]), // removed in PHP 7.2
    ("mdecrypt", &["openssl_", "sodium_"]), // mcrypt function with a different prefix
    ("ereg", &["preg_"]),              // removed in PHP 7.0
];

/// Second pass for removed extensions: code written with a modern alternative alongside the old
/// one is a fallback branch (`if (mysqli) {...} else { mysql_* }`). Such code cannot have run on
/// PHP 7+ without the fallback, so these findings are downgraded:
///  - same file has a guard for the extension or uses the modern alternative -> "info"
///  - another file in the package does -> "warning" with low confidence ("likely fallback")
pub fn apply_fallbacks(root: &Path, findings: &mut [crate::model::Finding]) {
    let mut file_cache: BTreeMap<String, String> = BTreeMap::new();
    let mut package_text: Option<String> = None;
    for f in findings.iter_mut().filter(|f| f.severity == "error") {
        let Some(ident) = rule_ident(&f.rule_id) else { continue };
        let key = ident.split('_').next().unwrap_or("").to_string();
        let Some((ext, alts)) = FALLBACKS.iter().find(|(e, _)| *e == key) else { continue };
        let guard = Regex::new(&format!(r#"(function_exists|extension_loaded|is_callable)\s*\(\s*['"]\\?{ext}"#)).unwrap();
        let text = file_cache
            .entry(f.file.clone())
            .or_insert_with(|| std::fs::read_to_string(root.join(&f.file)).unwrap_or_default().to_lowercase());
        let in_file = guard.is_match(text) || alts.iter().any(|a| text.contains(a));
        if in_file {
            f.severity = "info".into();
            f.confidence = "low".into();
            f.message = format!("Fallback code (this file also handles the modern alternative or checks that '{ext}' exists, so this path is skipped on newer PHP): {}", f.message);
            continue;
        }
        let pkg = package_text.get_or_insert_with(|| {
            let mut s = String::new();
            for e in WalkDir::new(root).into_iter().filter_map(|e| e.ok()).filter(|e| e.path().extension().map_or(false, |x| x == "php")) {
                if let Ok(t) = std::fs::read_to_string(e.path()) {
                    s.push_str(&t.to_lowercase());
                }
            }
            s
        });
        if guard.is_match(pkg) || alts.iter().any(|a| pkg.contains(a)) {
            f.severity = "warning".into();
            f.confidence = "low".into();
            f.message = format!("Likely fallback code (the package also uses the modern alternative or checks whether '{ext}' exists): {}", f.message);
        }
    }
}

#[derive(Deserialize)]
struct Suppression {
    rule_id: String,
    path_regex: String,
    /// "remove" (default): known false positive. "warning": real but non-breaking change.
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

/// Known false positives of the underlying rule set, kept as data in rules/suppressions.json.
pub fn apply_suppressions(rules_dir: &Path, findings: &mut Vec<crate::model::Finding>) {
    let Ok(Some(list)) = read_json::<Vec<Suppression>>(&rules_dir.join("suppressions.json")) else { return };
    let mut remove = vec![];
    let mut downgrade = vec![];
    for s in list {
        let Ok(re) = Regex::new(&s.path_regex) else { continue };
        if s.action.as_deref() == Some("warning") {
            downgrade.push((s.rule_id, re, s.reason.unwrap_or_default()));
        } else {
            remove.push((s.rule_id, re));
        }
    }
    findings.retain(|f| !remove.iter().any(|(rid, re)| f.rule_id == *rid && re.is_match(&f.file)));
    for f in findings.iter_mut().filter(|f| f.severity == "error") {
        if let Some((_, _, why)) = downgrade.iter().find(|(rid, re, _)| f.rule_id == *rid && re.is_match(&f.file)) {
            f.severity = "warning".into();
            f.message = format!("{} ({})", f.message, why);
        }
    }
}
