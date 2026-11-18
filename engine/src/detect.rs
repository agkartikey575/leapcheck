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

    for entry in WalkDir::new(root).sort_by_file_name().into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
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

/// Split a version into its numeric release part and a pre-release suffix ("6.0.0-rc1" -> [6,0,0], "rc1").
fn parse_ver(v: &str) -> (Vec<u64>, String) {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let cut = v.find(|c: char| c == '-' || c == '+' || c.is_ascii_alphabetic()).unwrap_or(v.len());
    let (rel, rest) = v.split_at(cut);
    let nums = rel.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).take(4).map(|s| s.parse().unwrap_or(0)).collect();
    let pre = if rest.starts_with('+') { String::new() } else { rest.trim_start_matches('-').to_lowercase() };
    (nums, pre)
}

/// Compare versions numerically; a pre-release (rc, beta, alpha...) sorts before its release.
pub fn ver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let ((x, xp), (y, yp)) = (parse_ver(a), parse_ver(b));
    for i in 0..x.len().max(y.len()) {
        let o = x.get(i).unwrap_or(&0).cmp(y.get(i).unwrap_or(&0));
        if o != Equal {
            return o;
        }
    }
    match (xp.is_empty(), yp.is_empty()) {
        (true, true) => Equal,
        (true, false) => Greater,
        (false, true) => Less,
        (false, false) => xp.cmp(&yp),
    }
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
    for entry in WalkDir::new(root).sort_by_file_name().into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        let p = entry.path();
        if p.extension().map_or(true, |e| e != "php") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(p) else { continue };
        let text = strip_comments(&raw);
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

/// Remove PHP comments (/* */ blocks, // and # line comments, keeping #[attributes]) so that
/// words inside comments never count as guards, alternatives or requirements. Line count is kept.
pub fn strip_comments(text: &str) -> String {
    let block = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let no_blocks = block.replace_all(text, |c: &regex::Captures| "\n".repeat(c[0].matches('\n').count()));
    let line_c = Regex::new(r"(^|[^:\\])//.*$|#[^\[].*$|#$").unwrap();
    no_blocks.lines().map(|l| line_c.replace(l, "$1").to_string()).collect::<Vec<_>>().join("\n")
}

/// Downgrade errors in code that only runs when the old feature exists, e.g. code inside
/// `if (function_exists('mcrypt_encrypt')) { ... }`, `extension_loaded('mysql')`, `defined('X')`,
/// or a PHP version check that selects old PHP. The guard must still be in scope at the finding:
/// either on the same line, the next line of a braceless `if`, or inside a `{` block it opened
/// that has not closed yet. Also recognised:
///  - the `else` branch of a check that selects NEW PHP (`if (PHP_VERSION_ID >= 70400) {} else { old }`)
///  - an early exit: `if (!function_exists('x')) { return; }` (or a new-PHP check followed by
///    `return`/`continue`/`break`) guards the rest of the enclosing block
///  - a short-circuit on one line: `!function_exists('x') || !x()`
///  - Strauss/PHP-Scoper prefixed names: `defined('Vendor\\Prefix\\X')`
///
/// Negated checks that end in `die`/`exit`/`throw` are not guards: the code fails on new PHP.
/// Guarded findings become severity "info" (shown, but not counted in the verdict).
/// This entry point handles errors only; the scan pipeline also runs it on warnings
/// (see `apply_guards_to`).
#[cfg(test)]
pub fn apply_guards(root: &Path, findings: &mut [crate::model::Finding]) {
    apply_guards_to(root, findings, &["error"]);
}

/// Lowest PHP version the engine checks against (PHP_VERSION_ID form). A version check only
/// counts as a guard when the old-PHP side of it lies entirely below this version.
const MIN_TARGET_ID: u64 = 80300;
/// PHP 8.0+ requires libxml2 2.9.0 or newer, so `LIBXML_VERSION < 20900` is never true there.
const MIN_LIBXML_ID: u64 = 20900;

#[derive(Clone, Copy, PartialEq, Debug)]
enum VerDir {
    /// The `if` branch runs only on old PHP.
    Old,
    /// The `if` branch runs only on new PHP; its `else` runs only on old PHP.
    New,
}

/// Where the finding line sits relative to a guard's `if` construct.
#[derive(PartialEq, Debug)]
enum Region {
    /// The condition statement has not ended (same multi-line expression).
    Unopened,
    /// Inside the `{ }` block the guard opened.
    InBlock,
    /// Inside the `else` / `elseif` part.
    InElse,
    /// After the whole construct; `scope_ok` = still in the same enclosing block.
    After { scope_ok: bool },
}

struct Walk {
    region: Region,
    /// The construct is an early exit: its body is only `return`/`continue`/`break`.
    early_exit: bool,
}

/// Follow braces from just after a guard (line `i`, byte `pos`) to the start of line `target`.
fn walk(lines: &[String], i: usize, pos: usize, target: usize) -> Walk {
    let exit_braced = Regex::new(r"^(return|continue|break)\b[^;{}]*;?$").unwrap();
    let exit_braceless = Regex::new(r"^[^;&{]*?\)\s*(return|continue|break)\b").unwrap();
    // phase 0: condition / braceless statement; 1: first block; 2: after it (looking for else);
    // 3: else block; 4: after the whole construct.
    let mut phase = 0u8;
    let mut depth: i64 = 0;
    let mut min_after: i64 = 0;
    let (mut cond, mut block, mut else_buf) = (String::new(), String::new(), String::new());
    let mut braceless = false;
    for (k, l) in lines.iter().enumerate().take(target).skip(i) {
        let seg = if k == i { &l[pos.min(l.len())..] } else { l.as_str() };
        for ch in seg.chars().chain(std::iter::once(' ')) {
            let mut again = true;
            while again {
                again = false;
                match phase {
                    0 => match ch {
                        '{' => {
                            phase = 1;
                            depth = 1;
                        }
                        ';' => {
                            cond.push(ch);
                            braceless = true;
                            phase = 2;
                        }
                        _ => cond.push(ch),
                    },
                    1 | 3 => {
                        match ch {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                        if depth == 0 {
                            phase = if phase == 1 { 2 } else { 4 };
                        } else if phase == 1 {
                            block.push(ch);
                        }
                    }
                    2 => {
                        if ch.is_whitespace() {
                            continue;
                        }
                        else_buf.push(ch);
                        if else_buf.len() <= 4 {
                            if !"else".starts_with(else_buf.as_str()) {
                                phase = 4;
                                again = true;
                            }
                        } else if ch == '{' {
                            phase = 3;
                            depth = 1;
                        } else if ch == ';' {
                            phase = 4;
                        }
                    }
                    _ => {
                        match ch {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                        min_after = min_after.min(depth);
                    }
                }
            }
        }
    }
    let region = match phase {
        0 => Region::Unopened,
        1 => Region::InBlock,
        2 if else_buf.starts_with("else") => Region::InElse,
        3 => Region::InElse,
        _ => Region::After { scope_ok: min_after >= 0 },
    };
    let early_exit = if braceless {
        exit_braceless.is_match(cond.trim())
    } else {
        !cond.contains("&&") && exit_braced.is_match(block.trim())
    };
    Walk { region, early_exit }
}

/// Like `apply_guards`, for the given severities (the scan pipeline uses errors and warnings).
pub fn apply_guards_to(root: &Path, findings: &mut [crate::model::Finding], severities: &[&str]) {
    let version_guard = Regex::new(r"(?i)((php_version_id|php_major_version|libxml_version)\s*([<>]=?|===?)|version_compare\s*\(\s*(php_version|phpversion\s*\(\s*\)))").unwrap();
    let mut cache: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in findings.iter_mut().filter(|f| severities.contains(&f.severity.as_str())) {
        let lines = cache.entry(f.file.clone()).or_insert_with(|| {
            std::fs::read_to_string(root.join(&f.file))
                .map(|t| strip_comments(&t).lines().map(|l| l.to_lowercase()).collect())
                .unwrap_or_default()
        });
        if lines.is_empty() || f.line == 0 {
            continue;
        }
        let target = (f.line as usize).min(lines.len()) - 1; // 0-based index of the finding line
        let start = target.saturating_sub(80);
        // A deprecated feature still exists on the target, so `function_exists('x')` / `defined('X')`
        // do not skip code that uses it: for warnings only PHP version checks count as guards.
        let ident = if f.severity == "error" { rule_ident(&f.rule_id) } else { None };
        let ident_re = ident.as_ref().map(|id| {
            Regex::new(&format!(
                r#"(!\s*)?\\?(function_exists|extension_loaded|defined|is_callable|class_exists)\s*\(\s*['"]\\?(?:[a-z0-9_]+\\{{1,2}})*{}"#,
                regex::escape(id)
            ))
            .unwrap()
        });
        let mut guarded = false;

        // Short-circuit on the finding line: `!function_exists('x') || ... x(...)`.
        if let Some(re) = &ident_re {
            let tail = Regex::new(r#"^[a-z0-9_]*['"]\s*\)\s*\)*\s*\|\|"#).unwrap();
            guarded = re.captures_iter(&lines[target]).any(|c| c.get(1).is_some() && tail.is_match(&lines[target][c.get(0).unwrap().end()..]));
        }

        for i in (start..=target).rev() {
            if guarded {
                break;
            }
            let line = &lines[i];
            // Guards on this line: positive and negated feature checks.
            let (mut pos_end, mut neg_end) = (None, None);
            if let Some(re) = &ident_re {
                for c in re.captures_iter(line) {
                    let end = c.get(0).unwrap().end();
                    if c.get(1).is_none() {
                        pos_end = Some(end);
                    } else {
                        neg_end = Some(end);
                    }
                }
            }
            let mut ver: Option<(usize, VerDir)> = None;
            if pos_end.is_none() {
                if let Some(m) = version_guard.find(line) {
                    let negated = line[..m.start()].trim_end_matches(|c: char| c.is_whitespace() || c == '(').ends_with('!');
                    if let Some(d) = version_dir(&line[m.start()..]) {
                        let d = if negated {
                            if d == VerDir::Old { VerDir::New } else { VerDir::Old }
                        } else {
                            d
                        };
                        ver = Some((m.end(), d));
                    }
                }
            }
            let distance = target - i;
            if let Some(pos) = pos_end.or(match ver {
                Some((p, VerDir::Old)) => Some(p),
                _ => None,
            }) {
                // Code inside the check runs only when the old feature exists.
                let is_version = pos_end.is_none();
                if i == target {
                    if !is_version || distance <= 15 {
                        guarded = true; // same line: ternary or one-line if
                        break;
                    }
                }
                let w = walk(lines, i, pos, target);
                let braceless_next = w.region != Region::InBlock && target == i + 1 && !lines[i].trim_end().ends_with(';');
                let near = !is_version || distance <= 15;
                match w.region {
                    Region::InBlock => {
                        guarded = true;
                        break;
                    }
                    Region::Unopened if near => {
                        guarded = true;
                        break;
                    }
                    _ if braceless_next && near => {
                        guarded = true;
                        break;
                    }
                    _ if near => break, // only the nearest guard is considered
                    _ => {}
                }
                continue;
            }
            // Negated feature check or a new-PHP check: protects its else branch, or the rest of
            // the block after an early exit.
            let alt = match (neg_end, ver) {
                (Some(p), _) => Some((p, false)),
                (None, Some((p, VerDir::New))) => Some((p, true)),
                _ => None,
            };
            if let Some((pos, else_ok)) = alt {
                if i == target {
                    continue;
                }
                let w = walk(lines, i, pos, target);
                if (else_ok && w.region == Region::InElse) || (w.early_exit && w.region == (Region::After { scope_ok: true })) {
                    guarded = true;
                }
            }
        }
        // Newer PHP features (NewConstants, NewFunctions, NewClasses...) with positive feature
        // detection for that exact name anywhere in the same file: the library selects this path
        // only when the feature exists (e.g. `case defined('X'):` in an engine switch).
        if !guarded && f.rule_id.contains(".New") {
            if let Some(re) = &ident_re {
                guarded = lines.iter().any(|l| re.captures_iter(l).any(|c| c.get(1).is_none()));
            }
        }
        if guarded {
            f.severity = "info".into();
            f.confidence = "low".into();
            f.message = format!("Guarded (only runs when the old feature is available, so it is skipped on newer PHP): {}", f.message);
        }
    }
}

/// Direction of a PHP version check that starts at `expr` (lower-case source), or None when it
/// is unknown or its old-PHP side could include a checked target version.
fn version_dir(expr: &str) -> Option<VerDir> {
    let e = expr.to_lowercase();
    // (old side is `<` / `<=`, the compared version id)
    let (op, id, min): (String, u64, u64);
    let konst = Regex::new(r"^(php_version_id|php_major_version|libxml_version)\s*(<=|<|>=|>|===|==)\s*(\d+)").unwrap();
    let vc3 = Regex::new(r#"^version_compare\s*\(\s*(?:php_version|phpversion\s*\(\s*\))\s*,\s*['"]([0-9.]+)['"]\s*,\s*['"]\s*(<=|<|lt|le|>=|>|ge|gt)\s*['"]"#).unwrap();
    let vc2 = Regex::new(r#"^version_compare\s*\(\s*(?:php_version|phpversion\s*\(\s*\))\s*,\s*['"]([0-9.]+)['"]\s*\)\s*(<=|<|>=|>|===|==)\s*(-1|0|1)\b"#).unwrap();
    let php_id = |v: &str| {
        let n: Vec<u64> = v.split('.').map(|x| x.parse().unwrap_or(0)).collect();
        n.first().unwrap_or(&0) * 10000 + n.get(1).unwrap_or(&0) * 100 + n.get(2).unwrap_or(&0)
    };
    if let Some(c) = konst.captures(&e) {
        let n: u64 = c[3].parse().ok()?;
        match &c[1] {
            "php_major_version" => {
                // Compare majors as version ids: `< 8` == `< 80000`, `<= 7` == `< 80000`.
                let (o, v) = match &c[2] {
                    "<" => ("<", n * 10000),
                    "<=" => ("<", (n + 1) * 10000),
                    ">=" => (">=", n * 10000),
                    ">" => (">=", (n + 1) * 10000),
                    _ if n <= 7 => ("<", (n + 1) * 10000), // == 7 selects only PHP 7
                    _ => return None,
                };
                op = o.into();
                id = v;
                min = MIN_TARGET_ID;
            }
            name => {
                if c[2].starts_with('=') {
                    return None;
                }
                op = c[2].to_string();
                id = n;
                min = if name == "libxml_version" { MIN_LIBXML_ID } else { MIN_TARGET_ID };
            }
        }
    } else if let Some(c) = vc3.captures(&e) {
        id = php_id(&c[1]);
        op = match &c[2] {
            "lt" => "<",
            "le" => "<=",
            "ge" => ">=",
            "gt" => ">",
            o => o,
        }
        .into();
        min = MIN_TARGET_ID;
    } else if let Some(c) = vc2.captures(&e) {
        // version_compare(PHP_VERSION, 'X') returns -1, 0 or 1.
        id = php_id(&c[1]);
        op = match (&c[2], &c[3]) {
            ("<", "0") | ("==" | "===", "-1") | ("<", "1") | ("<=", "-1") => "<",
            ("<=", "0") => "<=",
            (">=", "0") | (">", "-1") => ">=",
            (">", "0") | ("==" | "===", "1") | (">=", "1") => ">",
            _ => return None,
        }
        .into();
        min = MIN_TARGET_ID;
    } else {
        return None;
    }
    // The old side must lie entirely below the lowest checked target.
    match op.as_str() {
        "<" if id <= min => Some(VerDir::Old),
        "<=" if id < min => Some(VerDir::Old),
        ">=" if id <= min => Some(VerDir::New),
        ">" if id < min => Some(VerDir::New),
        _ => None,
    }
}

/// Old extensions removed from PHP long ago, and the modern alternatives code switches to.
const FALLBACKS: &[(&str, &[&str])] = &[
    ("mysql", &["mysqli"]),            // removed in PHP 7.0
    ("mcrypt", &["openssl_", "sodium_"]), // removed in PHP 7.2
    ("mdecrypt", &["openssl_", "sodium_"]), // mcrypt function with a different prefix
    ("ereg", &["preg_"]),              // removed in PHP 7.0
    ("eregi", &["preg_"]),             // removed in PHP 7.0
    ("split", &["preg_split", "explode"]), // removed in PHP 7.0
    ("spliti", &["preg_split"]),       // removed in PHP 7.0
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
            .or_insert_with(|| strip_comments(&std::fs::read_to_string(root.join(&f.file)).unwrap_or_default()).to_lowercase());
        let in_file = guard.is_match(text) || alts.iter().any(|a| text.contains(a));
        if in_file {
            f.severity = "info".into();
            f.confidence = "low".into();
            f.message = format!("Fallback code (this file also handles the modern alternative or checks that '{ext}' exists, so this path is skipped on newer PHP): {}", f.message);
            continue;
        }
        let pkg = package_text.get_or_insert_with(|| {
            let mut s = String::new();
            for e in WalkDir::new(root).sort_by_file_name().into_iter().filter_map(|e| e.ok()).filter(|e| e.path().extension().map_or(false, |x| x == "php")) {
                if let Ok(t) = std::fs::read_to_string(e.path()) {
                    s.push_str(&strip_comments(&t).to_lowercase());
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
    /// Exact rule ID, or a prefix ending in `*` (e.g. "PHPCompatibility.IniDirectives.RemovedIniDirectives.*").
    rule_id: String,
    path_regex: String,
    /// "remove" (default): known false positive.
    /// "warning": real but non-breaking change (errors become warnings).
    /// "info": not a change caused by the upgrade (errors and warnings become info).
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

fn rule_matches(pattern: &str, rule_id: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => rule_id.starts_with(prefix),
        None => rule_id == pattern,
    }
}

/// Known false positives and non-breaking rules of the underlying rule set, kept as data in
/// rules/suppressions.json. The first entry that matches a finding (rule and path) decides.
pub fn apply_suppressions(rules_dir: &Path, findings: &mut Vec<crate::model::Finding>) {
    let Ok(Some(list)) = read_json::<Vec<Suppression>>(&rules_dir.join("suppressions.json")) else { return };
    let entries: Vec<(String, Regex, String, String)> = list
        .into_iter()
        .filter_map(|s| {
            let re = Regex::new(&s.path_regex).ok()?;
            Some((s.rule_id, re, s.action.unwrap_or_else(|| "remove".into()), s.reason.unwrap_or_default()))
        })
        .collect();
    let find = |f: &crate::model::Finding| entries.iter().find(|(rid, re, _, _)| rule_matches(rid, &f.rule_id) && re.is_match(&f.file));
    findings.retain(|f| !matches!(find(f), Some((_, _, action, _)) if action == "remove"));
    for f in findings.iter_mut() {
        let Some((_, _, action, why)) = find(f) else { continue };
        let new_sev = match (action.as_str(), f.severity.as_str()) {
            ("warning", "error") => "warning",
            ("info", "error" | "warning") => "info",
            _ => continue,
        };
        f.severity = new_sev.into();
        f.message = format!("{} ({})", f.message, why);
    }
}

/// Matches the underlying rule set reports that are wrong for the code they point at:
///  - `ForbiddenBreakContinueOutsideLoop` when the line (or the line before) opens a braceless
///    loop body: `for (...) if (...) { break; }` is inside the loop.
///  - `RemovedFunctions.X` where X is a function declared in the file's own namespace somewhere in
///    the package (e.g. `GuzzleHttp\Promise\each()`): the call resolves to that function.
pub fn drop_rule_misreads(root: &Path, findings: &mut Vec<crate::model::Finding>) {
    let loop_re = Regex::new(r"\b(for|foreach|while)\s*\(").unwrap();
    let loop_head = Regex::new(r"^\s*(for|foreach|while)\s*\(.*\)\s*$").unwrap();
    let ns_re = Regex::new(r"(?m)^\s*namespace\s+([A-Za-z0-9_\\]+)\s*[;{]").unwrap();
    let fn_re = Regex::new(r"(?m)^\s*function\s+&?\s*([A-Za-z0-9_]+)\s*\(").unwrap();
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut ns_functions: Option<BTreeMap<String, std::collections::BTreeSet<String>>> = None;
    let mut text = |file: &str| -> String {
        files.entry(file.to_string()).or_insert_with(|| strip_comments(&std::fs::read_to_string(root.join(file)).unwrap_or_default())).clone()
    };
    findings.retain(|f| {
        if f.rule_id.contains("ForbiddenBreakContinueOutsideLoop") && f.line > 0 {
            let t = text(&f.file);
            let lines: Vec<&str> = t.lines().collect();
            let i = f.line as usize - 1;
            if let Some(l) = lines.get(i) {
                let kw = l.find("break").into_iter().chain(l.find("continue")).min();
                let same_line = loop_re.find(l).map_or(false, |m| kw.map_or(true, |k| m.start() < k));
                let prev_line = i > 0 && lines.get(i - 1).map_or(false, |p| loop_head.is_match(p));
                if same_line || prev_line {
                    return false;
                }
            }
            return true;
        }
        if f.rule_id.contains(".RemovedFunctions.") {
            let Some(name) = rule_ident(&f.rule_id) else { return true };
            let t = text(&f.file);
            let Some(ns) = ns_re.captures(&t).map(|c| c[1].to_lowercase()) else { return true };
            let decls = ns_functions.get_or_insert_with(|| namespaced_functions(root, &ns_re, &fn_re));
            return !decls.get(&ns).map_or(false, |s| s.contains(&name));
        }
        true
    });
}

/// Functions declared at namespace level in the package's PHP files, by lower-case namespace.
fn namespaced_functions(root: &Path, ns_re: &Regex, fn_re: &Regex) -> BTreeMap<String, std::collections::BTreeSet<String>> {
    let mut out: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for e in WalkDir::new(root).into_iter().filter_map(|e| e.ok()).filter(|e| e.path().extension().map_or(false, |x| x == "php" || x == "inc")) {
        let Ok(raw) = std::fs::read_to_string(e.path()) else { continue };
        let t = strip_comments(&raw);
        let Some(ns) = ns_re.captures(&t).map(|c| c[1].to_lowercase()) else { continue };
        // Only functions outside classes/interfaces/traits: skip files declaring one.
        let has_type = Regex::new(r"(?mi)^\s*(abstract\s+|final\s+)?(class|interface|trait|enum)\s+[A-Za-z_]").unwrap().is_match(&t);
        if has_type {
            continue;
        }
        let set = out.entry(ns).or_default();
        for c in fn_re.captures_iter(&t) {
            set.insert(c[1].to_lowercase());
        }
    }
    out
}

#[derive(Deserialize)]
struct Polyfill {
    function: String,
    wp_since: String,
}

/// WordPress "Requires at least" from the plugin/theme header (root PHP files or style.css), or
/// readme.txt when the header has none.
pub fn wp_requires_at_least(root: &Path) -> Option<String> {
    let re = Regex::new(r"(?im)^[\s*#@/]*Requires at least:\s*([0-9]+(?:\.[0-9]+)*)").unwrap();
    let mut header = None;
    if let Ok(rd) = std::fs::read_dir(root) {
        let mut paths: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !(name.ends_with(".php") || name == "style.css") || !p.is_file() {
                continue;
            }
            let Some(h) = head(&p) else { continue };
            let h: String = h.chars().take(8192).collect();
            if !(h.contains("Plugin Name:") || h.contains("Theme Name:")) {
                continue;
            }
            if let Some(c) = re.captures(&h) {
                header = Some(c[1].to_string());
                break;
            }
        }
    }
    header.or_else(|| {
        ["readme.txt", "README.txt", "readme.md", "README.md"]
            .iter()
            .find_map(|n| head(&root.join(n)).and_then(|h| re.captures(&h).map(|c| c[1].to_string())))
    })
}

/// New PHP functions WordPress core defines itself when PHP lacks them (wp-includes/compat.php,
/// data in rules/wp-polyfills.json). When the package requires a WordPress version that ships
/// the polyfill, the call works on any PHP: info. Otherwise it needs that WordPress version on
/// older PHP: warning.
pub fn apply_wp_polyfills(root: &Path, rules_dir: &Path, findings: &mut [crate::model::Finding]) {
    let Ok(Some(list)) = read_json::<Vec<Polyfill>>(&rules_dir.join("wp-polyfills.json")) else { return };
    let mut requires: Option<Option<String>> = None;
    for f in findings.iter_mut().filter(|f| (f.severity == "error" || f.severity == "warning") && f.rule_id.contains(".NewFunctions.")) {
        let Some(name) = rule_ident(&f.rule_id) else { continue };
        let Some(p) = list.iter().find(|p| p.function.to_lowercase() == name) else { continue };
        let req = requires.get_or_insert_with(|| wp_requires_at_least(root));
        if req.as_deref().map_or(false, |r| !ver_cmp(r, &p.wp_since).is_lt()) {
            f.severity = "info".into();
            f.confidence = "low".into();
            f.message = format!("WordPress {}+ provides {}() on every PHP version, and this package requires WordPress {}: {}", p.wp_since, p.function, req.as_deref().unwrap_or(""), f.message);
        } else {
            f.severity = "warning".into();
            f.confidence = "medium".into();
            f.message = format!("Needs WordPress {}+ (which provides {}()) on this PHP version: {}", p.wp_since, p.function, f.message);
        }
    }
}

/// `RemovedExtensions` matches name prefixes (`mysql_`, `fam`...), so it also hits WordPress's own
/// `mysql_to_rfc3339()`, static methods like `::mysql_time()` and plugin prefixes like `famne_`.
/// Every real call to a removed extension is also reported by the exact-name rules
/// (`RemovedFunctions` / `RemovedConstants`) on the same line, so:
///  - unpaired extension findings are dropped;
///  - paired ones become info (the exact-name finding already carries the verdict).
pub fn pair_extension_findings(findings: &mut Vec<crate::model::Finding>) {
    let ext = |f: &crate::model::Finding| f.rule_id.contains(".Extensions.RemovedExtensions.");
    let exact: std::collections::BTreeSet<(String, u32)> = findings
        .iter()
        .filter(|f| f.rule_id.contains(".RemovedFunctions.") || f.rule_id.contains(".RemovedConstants."))
        .map(|f| (f.file.clone(), f.line))
        .collect();
    findings.retain(|f| !ext(f) || exact.contains(&(f.file.clone(), f.line)));
    for f in findings.iter_mut().filter(|f| ext(f) && f.severity != "info") {
        f.severity = "info".into();
        f.message = format!("Same call as the removed function/constant reported on this line: {}", f.message);
    }
}

/// Drop findings reported more than once (same rule, file, line and message).
pub fn dedupe_findings(findings: &mut Vec<crate::model::Finding>) {
    let mut seen = std::collections::BTreeSet::new();
    findings.retain(|f| seen.insert((f.rule_id.clone(), f.file.clone(), f.line, f.message.clone())));
}

/// Files that are only loaded on old PHP: every line in the package that names the file picks it
/// on the old-PHP side of a version check on that same line, e.g.
/// `require(version_compare(PHP_VERSION, '5.3', '<') ? 'x-php52.php' : 'x.php')` or
/// `if (PHP_VERSION_ID < 50300) require 'x-php52.php';`. Their findings become info.
pub fn apply_version_selected_files(root: &Path, findings: &mut [crate::model::Finding]) {
    let version_guard = Regex::new(r"(?i)((php_version_id|php_major_version)\s*([<>]=?|===?)|version_compare\s*\(\s*(php_version|phpversion\s*\(\s*\)))").unwrap();
    let mut package: Option<Vec<(String, String)>> = None;
    let mut decided: BTreeMap<String, bool> = BTreeMap::new();
    for f in findings.iter_mut().filter(|f| f.severity == "error" || f.severity == "warning") {
        let old_only = *decided.entry(f.file.clone()).or_insert_with(|| {
            let base = f.file.rsplit('/').next().unwrap_or(&f.file).to_lowercase();
            let name_re = Regex::new(&format!(r"(^|[^a-z0-9_.\-]){}['\x22]", regex::escape(&base))).unwrap();
            let pkg = package.get_or_insert_with(|| {
                WalkDir::new(root)
                    .sort_by_file_name()
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map_or(false, |x| x == "php" || x == "inc"))
                    .filter_map(|e| std::fs::read_to_string(e.path()).ok().map(|t| (rel(root, e.path()), strip_comments(&t).to_lowercase())))
                    .collect()
            });
            let mut refs = 0;
            for (file, text) in pkg.iter() {
                if *file == f.file {
                    continue;
                }
                for line in text.lines() {
                    let Some(m) = name_re.find(line) else { continue };
                    refs += 1;
                    let Some(v) = version_guard.find(line[..m.start()].as_ref()) else { return false };
                    let negated = line[..v.start()].trim_end_matches(|c: char| c.is_whitespace() || c == '(').ends_with('!');
                    let Some(dir) = version_dir(&line[v.start()..]) else { return false };
                    let old_if = (dir == VerDir::Old) != negated;
                    let between = &line[v.end()..m.start()];
                    // Ternary: `cond ? A : B`. Statement form: `if (cond) require 'A';`.
                    let in_else = match between.find('?') {
                        Some(q) => between[q + 1..].replace("::", "").contains(':'),
                        None if !between.contains(';') && !between.contains('{') && !between.contains('}') => false,
                        None => return false,
                    };
                    if old_if == in_else {
                        return false;
                    }
                }
            }
            refs > 0
        });
        if old_only {
            f.severity = "info".into();
            f.confidence = "low".into();
            f.message = format!("File only loaded on old PHP (every include of it is selected by a PHP version check): {}", f.message);
        }
    }
}

/// Every post-processing step the scanner runs on analyzer output, in order.
pub fn post_process(root: &Path, rules_dir: &Path, findings: &mut Vec<crate::model::Finding>) {
    dedupe_findings(findings);
    apply_suppressions(rules_dir, findings);
    drop_rule_misreads(root, findings);
    apply_guards_to(root, findings, &["error", "warning"]);
    apply_version_selected_files(root, findings);
    apply_fallbacks(root, findings);
    apply_wp_polyfills(root, rules_dir, findings);
    pair_extension_findings(findings);
}

/// Name a component is known by across detectors: `guzzlehttp/guzzle` -> `guzzle`.
fn component_key(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_lowercase()
}

/// One entry per library copy:
///  - the same library found by a signature and by composer (`guzzle 7` next to
///    `guzzlehttp/guzzle 7.10.3`) keeps the composer entry when the signature's path is inside it
///    and its version (if any) is a prefix of the composer version;
///  - the same name and version found in several files keeps the shortest path.
/// Call after attribution: findings keep the deepest component path.
pub fn dedupe_components(comps: Vec<Component>) -> Vec<Component> {
    let composer: Vec<Component> = comps.iter().filter(|c| c.detection == "composer-installed").cloned().collect();
    let covered = |c: &Component| {
        c.detection == "signature"
            && composer.iter().any(|p| {
                component_key(&p.name) == component_key(&c.name)
                    && (c.path == p.path || c.path.starts_with(&format!("{}/", p.path)))
                    && match (&c.version, &p.version) {
                        (None, _) => true,
                        (Some(a), Some(b)) => b == a || b.starts_with(&format!("{a}.")),
                        (Some(_), None) => false,
                    }
            })
    };
    let mut out: BTreeMap<(String, Option<String>, String), Component> = BTreeMap::new();
    for c in comps.iter().filter(|c| !covered(c)) {
        let key = (c.name.clone(), c.version.clone(), c.kind.clone());
        match out.get_mut(&key) {
            Some(e) => {
                let vulns = e.known_vulnerabilities.max(c.known_vulnerabilities);
                if c.path.len() < e.path.len() || (c.path.len() == e.path.len() && c.path < e.path) {
                    *e = c.clone();
                }
                e.known_vulnerabilities = vulns;
            }
            None => {
                out.insert(key, c.clone());
            }
        }
    }
    let mut v: Vec<Component> = out.into_values().collect();
    v.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));
    v
}
