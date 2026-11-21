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
    /// Optional: the first 64 KB of the matched file must also match this regex (for libraries
    /// whose file names are generic, e.g. `ftp.php`).
    #[serde(default)]
    content_regex: Option<String>,
    /// Optional: the component path is this many folders above the matched file's folder (for
    /// libraries whose version file sits below the library root).
    #[serde(default)]
    root_up: usize,
}

/// One compiled signature rule.
struct Signature {
    name: String,
    file: Regex,
    version: Regex,
    content: Option<Regex>,
    root_up: usize,
}

#[derive(Deserialize)]
struct ExtensionRule {
    extension: String,
    patterns: Vec<String>,
}

#[derive(Deserialize)]
struct ConfirmedBreak {
    package: String,
    rule_id: String,
    path_regex: String,
}

/// An error checked by hand in one exact package version and found never to run
/// (rules/not-reached.json). A new version of the package is checked again automatically.
#[derive(Deserialize, Clone, Debug)]
pub struct NotReached {
    pub slug: String,
    pub version: String,
    /// File path relative to the package root.
    pub file: String,
    /// Exact rule ID.
    pub rule_id: String,
    pub reason: String,
    #[allow(dead_code)]
    pub reviewed_by_date: String,
}

pub struct Rules {
    signatures: Vec<Signature>,
    extensions: Vec<(String, Vec<Regex>)>,
    retire: Option<serde_json::Value>,
    /// Errors in bundled library code that were checked by hand and are reached by the
    /// package (package, rule pattern, path): they keep the "breaks" verdict.
    confirmed: Vec<(String, String, Regex)>,
    /// Errors checked by hand and never reached, per exact package version.
    not_reached: Vec<NotReached>,
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
            let content = s.content_regex.as_deref().map(Regex::new).transpose();
            match (Regex::new(&s.match_file_regex), Regex::new(&s.version_regex), content) {
                (Ok(file), Ok(version), Ok(content)) => signatures.push(Signature { name: s.name, file, version, content, root_up: s.root_up }),
                _ => eprintln!("{{\"level\":\"warn\",\"msg\":\"invalid signature rule\",\"rule\":\"{}\"}}", s.name),
            }
        }
        let extensions = exts
            .into_iter()
            .map(|e| (e.extension, e.patterns.iter().filter_map(|p| Regex::new(p).ok()).collect()))
            .collect();
        let confirmed_rules: Vec<ConfirmedBreak> = read_json(&dir.join("confirmed-breaks.json"))?.unwrap_or_default();
        let mut confirmed = vec![];
        for c in confirmed_rules {
            match Regex::new(&c.path_regex) {
                Ok(re) => confirmed.push((c.package, c.rule_id, re)),
                Err(_) => eprintln!("{{\"level\":\"warn\",\"msg\":\"invalid confirmed-break rule\",\"package\":\"{}\"}}", c.package),
            }
        }
        let not_reached: Vec<NotReached> = read_json(&dir.join("not-reached.json"))?.unwrap_or_default();
        Ok(Rules { signatures, extensions, retire, confirmed, not_reached })
    }

    pub fn summary(&self) -> String {
        format!(
            "signatures={} extensions={} retirejs={} confirmed_breaks={} not_reached={}",
            self.signatures.len(),
            self.extensions.len(),
            self.retire.is_some(),
            self.confirmed.len(),
            self.not_reached.len()
        )
    }

    /// Errors listed in rules/not-reached.json for this exact package version (slug, version,
    /// file and rule ID) become info, with the reviewer's reason in the message.
    pub fn apply_not_reached(&self, package: &str, version: &str, findings: &mut [crate::model::Finding]) {
        for f in findings.iter_mut().filter(|f| f.severity == "error") {
            if let Some(n) = self.not_reached.iter().find(|n| n.slug == package && n.version == version && n.file == f.file && n.rule_id == f.rule_id) {
                f.severity = "info".into();
                f.confidence = "low".into();
                f.message = format!("Checked by hand, this code is not reached ({}): {}", n.reason, f.message);
            }
        }
    }

    /// A bundled-library error in `package` that was checked by hand and is reachable.
    pub fn is_confirmed_break(&self, package: &str, f: &crate::model::Finding) -> bool {
        self.confirmed.iter().any(|(p, rid, re)| p == package && rule_matches(rid, &f.rule_id) && re.is_match(&f.file))
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
            for sig in &rules.signatures {
                if sig.file.is_match(&r) {
                    let text = head(p);
                    if let Some(c) = &sig.content {
                        if !text.as_deref().map_or(false, |t| c.is_match(t)) {
                            continue;
                        }
                    }
                    let version = text.and_then(|h| sig.version.captures(&h).and_then(|c| c.get(1)).map(|m| m.as_str().to_string()));
                    let mut dir = r.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                    for _ in 0..sig.root_up {
                        dir = dir.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                    }
                    if sig.root_up > 0 && dir.is_empty() {
                        continue; // a library root above its folder cannot be the whole package
                    }
                    add(Component { name: sig.name.clone(), version, kind: "php".into(), path: dir, detection: "signature".into(), known_vulnerabilities: 0 });
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
/// A small lexer: comment markers inside strings (`'/*'`, `"#fff"`, `'http://'`), heredocs and
/// HTML outside `<?php ... ?>` are left alone.
pub fn strip_comments(text: &str) -> String {
    #[derive(PartialEq)]
    enum St {
        Html,
        Code,
        Quote(char),
        Line,
        Block,
        Heredoc(String),
    }
    let c: Vec<char> = text.chars().collect();
    let n = c.len();
    let at = |i: usize, s: &str| s.chars().enumerate().all(|(k, ch)| c.get(i + k) == Some(&ch));
    let ident = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut out = String::with_capacity(text.len());
    let mut st = St::Html;
    let mut i = 0;
    while i < n {
        let ch = c[i];
        match &st {
            St::Html => {
                if at(i, "<?") {
                    st = St::Code;
                    out.push_str("<?");
                    i += 2;
                    continue;
                }
                out.push(ch);
            }
            St::Code => {
                if at(i, "?>") {
                    st = St::Html;
                    out.push_str("?>");
                    i += 2;
                    continue;
                }
                match ch {
                    '\'' | '"' | '`' => {
                        st = St::Quote(ch);
                        out.push(ch);
                    }
                    '/' if c.get(i + 1) == Some(&'/') => st = St::Line,
                    '/' if c.get(i + 1) == Some(&'*') => {
                        st = St::Block;
                        i += 2;
                        continue;
                    }
                    '#' if c.get(i + 1) != Some(&'[') => st = St::Line,
                    '<' if at(i, "<<<") => {
                        let mut j = i + 3;
                        while j < n && (c[j] == ' ' || c[j] == '\t') {
                            j += 1;
                        }
                        if j < n && (c[j] == '\'' || c[j] == '"') {
                            j += 1;
                        }
                        let s0 = j;
                        while j < n && ident(c[j]) {
                            j += 1;
                        }
                        let id: String = c[s0..j].iter().collect();
                        out.extend(&c[i..j]);
                        i = j;
                        if !id.is_empty() {
                            st = St::Heredoc(id);
                        }
                        continue;
                    }
                    _ => out.push(ch),
                }
            }
            St::Quote(q) => {
                out.push(ch);
                if ch == '\\' && i + 1 < n {
                    out.push(c[i + 1]);
                    i += 2;
                    continue;
                }
                if ch == *q {
                    st = St::Code;
                }
            }
            St::Line => {
                if ch == '\n' {
                    out.push('\n');
                    st = St::Code;
                } else if at(i, "?>") {
                    st = St::Code; // `?>` ends a line comment; handled by Code next round
                    continue;
                }
            }
            St::Block => {
                if at(i, "*/") {
                    st = St::Code;
                    i += 2;
                    continue;
                }
                if ch == '\n' {
                    out.push('\n');
                }
            }
            St::Heredoc(id) => {
                out.push(ch);
                if ch == '\n' {
                    // The closing marker may be indented (PHP 7.3+).
                    let mut j = i + 1;
                    while j < n && (c[j] == ' ' || c[j] == '\t') {
                        j += 1;
                    }
                    if at(j, id) && !c.get(j + id.chars().count()).map_or(false, |&x| ident(x)) {
                        st = St::Code;
                    }
                }
            }
        }
        i += 1;
    }
    out
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

/// Start of a PHP version check (regex, lower-case source): `PHP_VERSION_ID <`,
/// `version_compare(PHP_VERSION`, `strnatcmp(phpversion()`. Parsed by `version_check`.
const VERSION_GUARD: &str = r"(php_version_id|php_major_version)\s*([<>]=?|===?)|(version_compare|strnatcmp)\s*\(\s*(php_version|phpversion\s*\(\s*\))";

/// Core PHP functions (always compiled in, never in an optional extension) and the PHP version
/// that added them, as a version id. `function_exists()` of one of them is false only on older PHP.
const CORE_FUNCTIONS_SINCE: &[(&str, u64)] = &[
    ("str_contains", 80000),
    ("str_starts_with", 80000),
    ("str_ends_with", 80000),
    ("get_debug_type", 80000),
    ("array_is_list", 80100),
    ("json_validate", 80300),
    ("stream_context_set_options", 80300),
    ("array_find", 80400),
    ("array_find_key", 80400),
    ("array_any", 80400),
    ("array_all", 80400),
    ("array_first", 80500),
    ("array_last", 80500),
];

fn core_function_since(name: &str) -> Option<u64> {
    CORE_FUNCTIONS_SINCE.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

/// Lowest PHP version id named in a "deprecated since PHP X.Y" / "removed since PHP X.Y" message.
fn changed_since_id(message: &str) -> Option<u64> {
    let re = Regex::new(r"(?i)\b(?:deprecated|removed)\s+since\s+(?:php\s*)?(\d+)\.(\d+)").unwrap();
    re.captures_iter(message).filter_map(|c| Some(c[1].parse::<u64>().ok()? * 10000 + c[2].parse::<u64>().ok()? * 100)).min()
}

/// For a ternary whose condition starts at byte `start` of `text` (`cond ? a : b`), the byte
/// ranges of its then part and else part. None when the text is not the condition of a ternary:
/// the statement ends or a block opens first, the condition is combined with another one
/// (`&&`, `||`, `and`, `or`, `xor`), is one argument of a call, or uses the short form `?:`.
pub fn ternary_parts(text: &str, start: usize) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let b = text.as_bytes();
    let (mut depth, mut floor) = (0i64, 0i64);
    let (mut phase, mut nested) = (0u8, 0u32);
    let (mut q, mut c) = (0usize, 0usize);
    let mut quote: Option<u8> = None;
    let mut i = start;
    let word = |i: usize, w: &[u8]| {
        b[i..].starts_with(w)
            && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'$'))
            && b.get(i + w.len()).map_or(true, |x| !(x.is_ascii_alphanumeric() || *x == b'_'))
    };
    while i < b.len() {
        let ch = b[i];
        if let Some(qc) = quote {
            if ch == b'\\' {
                i += 2;
                continue;
            }
            if ch == qc {
                quote = None;
            }
            i += 1;
            continue;
        }
        let next = b.get(i + 1).copied().unwrap_or(b' ');
        match ch {
            b'\'' | b'"' => quote = Some(ch),
            b'(' | b'[' | b'{' if phase > 0 || ch != b'{' => depth += 1,
            b'{' => return None,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth < floor {
                    match phase {
                        0 if ch == b')' => floor = depth,
                        2 => return Some((q + 1..c, c + 1..i)),
                        _ => return None,
                    }
                }
            }
            b';' if depth > floor => {}
            b';' if phase < 2 => return None,
            b';' | b',' if phase == 2 && depth == floor => return Some((q + 1..c, c + 1..i)),
            b',' if phase == 0 && depth == floor => return None,
            b'&' | b'|' if phase == 0 && next == ch => return None,
            b'a' | b'o' | b'x' if phase == 0 && (word(i, b"and") || word(i, b"or") || word(i, b"xor")) => return None,
            b'?' if next == b'?' || next == b'-' => i += 1,
            b'?' if depth == floor => match phase {
                0 if next == b':' => return None,
                0 => {
                    q = i;
                    phase = 1;
                }
                1 => nested += 1,
                _ => return Some((q + 1..c, c + 1..i)),
            },
            b'?' if phase == 0 => return None,
            b':' if next == b':' => i += 1,
            b':' if phase == 1 && depth == floor => {
                if nested > 0 {
                    nested -= 1;
                } else {
                    c = i;
                    phase = 2;
                }
            }
            _ => {}
        }
        i += 1;
    }
    (phase == 2).then(|| (q + 1..c, c + 1..b.len()))
}

/// Which side of a ternary the finding on line `target` sits on, when the version (or feature)
/// check starting at byte `start` of line `i` is that ternary's condition: Some(true) = only in
/// the then part, Some(false) = only in the else part, None = not a ternary or unclear. The
/// finding is located by the words on its line that start the rule's last name segment
/// (`e_strict` for `e_strictDeprecated`); a line that lies wholly in one part needs no word.
fn ternary_side(lines: &[String], i: usize, start: usize, target: usize, rule_id: &str) -> Option<bool> {
    let mut text = lines[i][start.min(lines[i].len())..].to_string();
    let mut line_off = 0;
    for l in &lines[i + 1..=target] {
        text.push('\n');
        line_off = text.len();
        text.push_str(l);
    }
    let (then_r, else_r) = ternary_parts(&text, 0)?;
    let line_end = text.len();
    if i == target && lines[i][..start.min(lines[i].len())].chars().any(|c| c.is_ascii_alphabetic()) {
        // Code before the check on the same line could hold the finding.
        let tail = rule_id.rsplit('.').next().unwrap_or("").to_lowercase();
        let words = Regex::new(r"[a-z_][a-z0-9_]*").unwrap();
        if words.find_iter(&lines[i][..start]).any(|w| w.as_str().len() >= 3 && tail.starts_with(w.as_str())) {
            return None;
        }
    }
    if i != target {
        let first = text[line_off..].find(|c: char| !c.is_whitespace()).map_or(line_end, |k| line_off + k);
        if else_r.start <= first && line_end <= else_r.end {
            return Some(false);
        }
        if then_r.start <= first && line_end <= then_r.end {
            return Some(true);
        }
    }
    let tail = rule_id.rsplit('.').next().unwrap_or("").to_lowercase();
    let words = Regex::new(r"[a-z_][a-z0-9_]*").unwrap();
    let hits: Vec<usize> = words
        .find_iter(&text[line_off..])
        .filter(|w| w.as_str().len() >= 3 && tail.starts_with(w.as_str()))
        .map(|w| line_off + w.start())
        .collect();
    if hits.is_empty() {
        return None;
    }
    if hits.iter().all(|h| else_r.contains(h)) {
        Some(false)
    } else if hits.iter().all(|h| then_r.contains(h)) {
        Some(true)
    } else {
        None
    }
}

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
    let version_guard = Regex::new(&format!("(?i)({VERSION_GUARD}|libxml_version\\s*([<>]=?|===?))")).unwrap();
    let dyn_exists = Regex::new(r"(!\s*)?\\?function_exists\s*\(\s*\$(?:this->)?([a-z0-9_]+)\s*\)").unwrap();
    let core_exists = Regex::new(r#"(!\s*)?\\?function_exists\s*\(\s*['"]\\{0,2}([a-z0-9_]+)['"]\s*\)"#).unwrap();
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
            let id = regex::escape(id);
            Regex::new(&format!(
                r#"(!\s*)?\\?(function_exists|extension_loaded|defined|is_callable|class_exists|interface_exists|enum_exists)\s*\(\s*(?:['"]\\{{0,2}}(?:[a-z0-9_]+\\{{1,2}})*{id}|\\?(?:[a-z0-9_]+\\)*{id}::class\b)"#
            ))
            .unwrap()
        });
        // Newer PHP features (NewClasses, NewFunctions...) are missing on the target; code that only
        // runs on a PHP version above every checked target never runs there.
        let new_feature = f.rule_id.contains(".New");
        // A removed or deprecated feature is only reported on targets at or above the version that
        // changed it, so a check whose old side lies below that version also skips it (E_STRICT,
        // deprecated in 8.4, behind `PHP_VERSION_ID >= 80400 ? ... : E_STRICT`).
        let floor = if new_feature { MIN_TARGET_ID } else { changed_since_id(&f.message).map_or(MIN_TARGET_ID, |v| v.max(MIN_TARGET_ID)) };
        // `function_exists('x')` of another function that merely starts with the rule's name
        // (`stream_context_set_options` for `stream_context_set_option`) is not a guard for it.
        let other_core = |line: &str, end: usize, id: &str| {
            let rest: String = line[end..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            !rest.is_empty() && core_function_since(&format!("{id}{rest}")).is_some()
        };
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
                    if other_core(line, end, ident.as_deref().unwrap_or("")) {
                        continue;
                    }
                    if c.get(1).is_none() {
                        pos_end = Some(end);
                    } else {
                        neg_end = Some(end);
                    }
                }
            }
            let mut ver: Option<(usize, VerDir)> = None;
            let mut new_only: Option<usize> = None;
            // Byte where the version check starts (for ternaries).
            let mut ver_start = 0;
            if pos_end.is_none() {
                if let Some(m) = version_guard.find(line) {
                    ver_start = m.start();
                    let negated = line[..m.start()].trim_end_matches(|c: char| c.is_whitespace() || c == '(').ends_with('!');
                    if new_feature && !negated && new_php_floor(&line[m.start()..]).map_or(false, |n| n > MIN_TARGET_ID) {
                        new_only = Some(m.end());
                    }
                    if let Some(d) = version_dir(&line[m.start()..], floor) {
                        let d = if negated {
                            if d == VerDir::Old { VerDir::New } else { VerDir::Old }
                        } else {
                            d
                        };
                        ver = Some((m.end(), d));
                    }
                }
                // `function_exists('f')` for a core function added in PHP X is false only below X,
                // so it works like `PHP_VERSION_ID >= X` for code that must not run on new PHP
                // (e.g. `function_exists('stream_context_set_options') ? ... : stream_context_set_option(...)`).
                if ver.is_none() && !new_feature {
                    for c in core_exists.captures_iter(line) {
                        let Some(since) = core_function_since(&c[2]) else { continue };
                        if since <= floor {
                            ver = Some((c.get(0).unwrap().end(), if c.get(1).is_some() { VerDir::Old } else { VerDir::New }));
                            ver_start = c.get(0).unwrap().start();
                        }
                    }
                }
            }
            let distance = target - i;
            if let Some(pos) = pos_end.or(new_only).or(match ver {
                Some((p, VerDir::Old)) => Some(p),
                _ => None,
            }) {
                // Code inside the check runs only when the old feature exists.
                let is_version = pos_end.is_none();
                // `old-PHP check ? a : <finding>`: the finding is on the new-PHP side.
                let other_side = is_version && ternary_side(lines, i, ver_start, target, &f.rule_id) == Some(false);
                if i == target {
                    if other_side {
                        break;
                    }
                    if !is_version || distance <= 15 {
                        guarded = true; // same line: ternary or one-line if
                        break;
                    }
                }
                let w = walk(lines, i, pos, target);
                if other_side && w.region == Region::Unopened {
                    break;
                }
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
                // `new-PHP check ? a : <finding>` (same line or a multi-line expression).
                let ternary_else = |w: Option<&Walk>| {
                    else_ok && w.map_or(true, |w| w.region == Region::Unopened) && distance <= 15 && ternary_side(lines, i, ver_start, target, &f.rule_id) == Some(false)
                };
                if i == target {
                    if ternary_else(None) {
                        guarded = true;
                    }
                    continue;
                }
                let w = walk(lines, i, pos, target);
                if (else_ok && w.region == Region::InElse) || (w.early_exit && w.region == (Region::After { scope_ok: true })) || ternary_else(Some(&w)) {
                    guarded = true;
                }
                continue;
            }
            // `if (function_exists($this->fn)) return;` where `$fn` defaults to a function that
            // always exists (php-markdown's `utf8_strlen = 'mb_strlen'`): the rest never runs.
            if f.severity == "error" && i != target {
                if let Some(c) = dyn_exists.captures(line) {
                    if c.get(1).is_none() && always_exists_default(lines, &c[2]) {
                        let w = walk(lines, i, c.get(0).unwrap().end(), target);
                        if w.early_exit && w.region == (Region::After { scope_ok: true }) {
                            guarded = true;
                        }
                    }
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
            f.message = if new_feature {
                format!("Guarded (only runs when PHP already has this feature, so it is skipped where it is missing): {}", f.message)
            } else {
                format!("Guarded (only runs when the old feature is available, so it is skipped on newer PHP): {}", f.message)
            };
        }
    }
}

/// Functions that exist on every PHP 8 site running WordPress (WordPress core polyfills
/// `mb_strlen`/`mb_substr` in wp-includes/compat.php).
const ALWAYS_EXISTS: &[&str] = &["mb_strlen", "mb_substr", "strlen", "substr", "strtolower", "strtoupper"];

/// True when the variable or property `name` is initialised in the file (lower-case lines) with the
/// name of a function that always exists, e.g. `public $utf8_strlen = 'mb_strlen';`.
fn always_exists_default(lines: &[String], name: &str) -> bool {
    let re = Regex::new(&format!(r#"\${}\s*=\s*['"]([a-z0-9_]+)['"]"#, regex::escape(name))).unwrap();
    let mut seen = false;
    for l in lines {
        for c in re.captures_iter(l) {
            if !ALWAYS_EXISTS.contains(&&c[1]) {
                return false; // assigned another function somewhere: not provably always there
            }
            seen = true;
        }
    }
    seen
}

/// Direction of a PHP version check that starts at `expr` (lower-case source), or None when it
/// is unknown or its old-PHP side could include a checked target version.
/// `floor` raises the PHP threshold for findings that only exist from a later version on.
fn version_dir(expr: &str, floor: u64) -> Option<VerDir> {
    let (op, id, min) = version_check(expr)?;
    let min = if min == MIN_TARGET_ID { min.max(floor) } else { min };
    // The old side must lie entirely below the lowest checked target.
    match op.as_str() {
        "<" if id <= min => Some(VerDir::Old),
        "<=" if id < min => Some(VerDir::Old),
        ">=" if id <= min => Some(VerDir::New),
        ">" if id < min => Some(VerDir::New),
        _ => None,
    }
}

/// For a check that selects new PHP (`PHP_VERSION_ID >= 80600`, `version_compare(PHP_VERSION,
/// '8.6', '>=')`), the lowest PHP version id on its new side.
fn new_php_floor(expr: &str) -> Option<u64> {
    let (op, id, min) = version_check(expr)?;
    if min != MIN_TARGET_ID {
        return None; // libxml checks say nothing about the PHP version
    }
    match op.as_str() {
        ">=" => Some(id),
        ">" => Some(id + 1),
        _ => None,
    }
}

/// Parse a PHP version check at the start of `expr`: (operator on the version id, version id,
/// lowest checked id for that kind of check).
fn version_check(expr: &str) -> Option<(String, u64, u64)> {
    let e = expr.to_lowercase();
    // (old side is `<` / `<=`, the compared version id)
    let (op, id, min): (String, u64, u64);
    let konst = Regex::new(r"^(php_version_id|php_major_version|libxml_version)\s*(<=|<|>=|>|===|==)\s*(\d+)").unwrap();
    let vc3 = Regex::new(r#"^version_compare\s*\(\s*(?:php_version|phpversion\s*\(\s*\))\s*,\s*['"]([0-9.]+)['"]\s*,\s*['"]\s*(<=|<|lt|le|>=|>|ge|gt)\s*['"]"#).unwrap();
    let vc2 = Regex::new(r#"^(?:version_compare|strnatcmp)\s*\(\s*(?:php_version|phpversion\s*\(\s*\))\s*,\s*['"]([0-9.]+)['"]\s*\)\s*(<=|<|>=|>|===|==)\s*(-1|0|1)\b"#).unwrap();
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
        // version_compare(PHP_VERSION, 'X') returns -1, 0 or 1; so does strnatcmp() on PHP 8.2+,
        // and its natural order matches version order for dotted numbers.
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
    Some((op, id, min))
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

/// Findings that depend on what the rest of the package does:
///  - `RemovedCallingDestructAfterConstructorExit`: only matters when a destructor exists; with
///    no `function __destruct` anywhere in the package it becomes info.
///  - `$HTTP_RAW_POST_DATA` in a file that assigns the variable itself (usually from
///    `php://input`) becomes info: the code never relies on PHP filling it.
///  - `NewFunctions.X` when the package declares its own global `X()` (a polyfill such as
///    symfony/polyfill-php85) or the file imports a namespaced `X` with `use function`: the call
///    works on every PHP version, so it becomes info.
pub fn apply_package_context(root: &Path, findings: &mut [crate::model::Finding]) {
    let destruct = Regex::new(r"(?i)\bfunction\s+&?\s*__destruct\s*\(").unwrap();
    let raw_assign = Regex::new(r"(?i)\$http_raw_post_data\s*=[^=]").unwrap();
    let mut package: Option<Vec<(String, String)>> = None;
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let load = |root: &Path| -> Vec<(String, String)> {
        WalkDir::new(root)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |x| x == "php" || x == "inc"))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok().map(|t| (rel(root, e.path()), strip_comments(&t))))
            .collect()
    };
    let mut globals: Option<std::collections::BTreeSet<String>> = None;
    for f in findings.iter_mut().filter(|f| f.severity == "error" || f.severity == "warning") {
        let why: Option<String> = if f.rule_id.contains("RemovedCallingDestructAfterConstructorExit") {
            let pkg = package.get_or_insert_with(|| load(root));
            (!pkg.iter().any(|(_, t)| destruct.is_match(t))).then(|| "no class in this package has a __destruct() method, so nothing changes".to_string())
        } else if f.rule_id.contains("RemovedPredefinedGlobalVariables.http_raw_post_data") {
            let t = files.entry(f.file.clone()).or_insert_with(|| strip_comments(&std::fs::read_to_string(root.join(&f.file)).unwrap_or_default()));
            raw_assign.is_match(t).then(|| "this file sets $HTTP_RAW_POST_DATA itself, so it does not rely on PHP to fill it".to_string())
        } else if f.rule_id.contains(".NewFunctions.") {
            match rule_ident(&f.rule_id) {
                Some(name) => {
                    let t = files.entry(f.file.clone()).or_insert_with(|| strip_comments(&std::fs::read_to_string(root.join(&f.file)).unwrap_or_default()));
                    let imported = imports_function(t, &name);
                    let own = imported
                        || globals
                            .get_or_insert_with(|| {
                                let pkg = package.get_or_insert_with(|| load(root));
                                global_functions(pkg)
                            })
                            .contains(&name);
                    own.then(|| format!("this package ships its own {name}(), so the call works on every PHP version"))
                }
                None => None,
            }
        } else {
            None
        };
        if let Some(why) = why {
            f.severity = "info".into();
            f.confidence = "low".into();
            f.message = format!("{} ({why})", f.message);
        }
    }
}

/// Lower-case names of functions declared in the global namespace: files with no `namespace`
/// and no class/interface/trait/enum declaration (polyfill bootstrap files, function libraries).
fn global_functions(pkg: &[(String, String)]) -> std::collections::BTreeSet<String> {
    let ns = Regex::new(r"(?mi)^\s*namespace\s+[A-Za-z0-9_\\]+\s*[;{]").unwrap();
    let ty = Regex::new(r"(?mi)^\s*(abstract\s+|final\s+|readonly\s+)*(class|interface|trait|enum)\s+[A-Za-z_]").unwrap();
    let func = Regex::new(r"(?i)\bfunction\s+&?\s*([a-z0-9_]+)\s*\(").unwrap();
    let mut out = std::collections::BTreeSet::new();
    for (_, t) in pkg.iter().filter(|(_, t)| !ns.is_match(t) && !ty.is_match(t)) {
        for c in func.captures_iter(t) {
            out.insert(c[1].to_lowercase());
        }
    }
    out
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
    /// Optional: the finding's message must also match this regex.
    #[serde(default)]
    message_regex: Option<String>,
}

/// Exact rule ID, or a pattern where `*` stands for any text (e.g. `A.B.*` or `A.B.*Removed`).
fn rule_matches(pattern: &str, rule_id: &str) -> bool {
    if !pattern.contains('*') {
        return rule_id == pattern;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !rule_id.starts_with(first) || rule_id.len() < first.len() + last.len() || !rule_id.ends_with(last) {
        return false;
    }
    let mut rest = &rule_id[first.len()..rule_id.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(k) => rest = &rest[k + mid.len()..],
            None => return false,
        }
    }
    true
}

/// Known false positives and non-breaking rules of the underlying rule set, kept as data in
/// rules/suppressions.json. The first entry that matches a finding (rule and path) decides.
pub fn apply_suppressions(rules_dir: &Path, findings: &mut Vec<crate::model::Finding>) {
    let Ok(Some(list)) = read_json::<Vec<Suppression>>(&rules_dir.join("suppressions.json")) else { return };
    let entries: Vec<(String, Regex, String, String, Option<Regex>)> = list
        .into_iter()
        .filter_map(|s| {
            let re = Regex::new(&s.path_regex).ok()?;
            let msg = s.message_regex.as_deref().map(Regex::new).transpose().ok()?;
            Some((s.rule_id, re, s.action.unwrap_or_else(|| "remove".into()), s.reason.unwrap_or_default(), msg))
        })
        .collect();
    let find = |f: &crate::model::Finding| {
        entries.iter().find(|(rid, re, _, _, msg)| rule_matches(rid, &f.rule_id) && re.is_match(&f.file) && msg.as_ref().map_or(true, |m| m.is_match(&f.message)))
    };
    findings.retain(|f| !matches!(find(f), Some((_, _, action, _, _)) if action == "remove"));
    for f in findings.iter_mut() {
        let Some((_, _, action, why, _)) = find(f) else { continue };
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
///  - `ForbiddenBreakContinueOutsideLoop` when the statement sits in a braceless loop body, also
///    through nested braceless `if`/`else`/`try` heads on earlier lines (`foreach (...)` newline
///    `if (...) { ...; break; }`, `while (true) try { ... continue; }`).
///  - `RemovedFunctions.X` where X is a function declared in the file's own namespace somewhere in
///    the package (e.g. `GuzzleHttp\Promise\each()`), or imported with `use function Ns\X;`: the
///    call resolves to that function.
///  - `ForbiddenThisUseContexts.Global` on `global ${$this->name}` (a dynamic variable name).
///  - `InternalInterfaces.datetimeinterfaceFound` on an interface that extends DateTimeInterface.
///  - `define()` with a third argument that is literally `false` (PHP 8 only rejects `true`).
///  - `InternalInterfaces.throwableFound` on `class X extends Y implements Throwable` (valid).
///  - `RemovedImplodeFlexibleParamOrder` when the second argument is an array literal.
/// It also lowers to info:
///  - `NewLateStaticBinding.OutsideClassScope` in a file that declares no class/trait/enum (a
///    template included from a class method);
///  - `RemovedOrphanedParent` on a line with no `parent::` (a bare `parent` word);
///  - `define()` whose third argument is a variable passed through (e.g. a wrapper function);
///  - `RemovedTernaryAssociativity` inside a `match` expression: such a file only compiles on
///    PHP 8, where a real unparenthesised nested ternary is a compile error, so the upgrade
///    changes nothing (the rule misreads parenthesised ternaries in match arms).
pub fn drop_rule_misreads(root: &Path, findings: &mut Vec<crate::model::Finding>) {
    let ns_re = Regex::new(r"(?m)^\s*namespace\s+([A-Za-z0-9_\\]+)\s*[;{]").unwrap();
    let fn_re = Regex::new(r"(?m)^\s*function\s+&?\s*([A-Za-z0-9_]+)\s*\(").unwrap();
    let global_re = Regex::new(r"(?i)\bglobal\s+([^;]*)").unwrap();
    let dyn_var = Regex::new(r"\$\{[^}]*\}").unwrap();
    let this_re = Regex::new(r"(?i)\$this\b").unwrap();
    let iface_re = Regex::new(r"(?i)\binterface\s+[a-z0-9_]+[^{;]*\bextends\b").unwrap();
    let decl_re = Regex::new(r"(?i)\b(class|interface|trait|enum)\s+[a-z0-9_]").unwrap();
    let throwable_sub = Regex::new(r"(?i)\bclass\s+[a-z0-9_]+\s+extends\s+\\?[a-z0-9_\\]+\s+implements\s+[^{;]*\bthrowable\b").unwrap();
    let type_decl = Regex::new(r"(?mi)^\s*(abstract\s+|final\s+|readonly\s+)*(class|trait|enum)\s+[a-z_]").unwrap();
    let implode_call = Regex::new(r"(?i)(^|[^a-z0-9_$>:\\])\\?(implode|join)\s*\(").unwrap();
    let parent_call = Regex::new(r"(?i)\bparent\s*::").unwrap();
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut ns_functions: Option<BTreeMap<String, std::collections::BTreeSet<String>>> = None;
    let mut text = |file: &str| -> String {
        files.entry(file.to_string()).or_insert_with(|| strip_comments(&std::fs::read_to_string(root.join(file)).unwrap_or_default())).clone()
    };
    let mut decide = |f: &crate::model::Finding| -> Misread {
        use Misread::*;
        if f.line == 0 {
            return Keep;
        }
        let i = f.line as usize - 1;
        if f.rule_id.contains("ForbiddenBreakContinueOutsideLoop") {
            let t = text(&f.file);
            let Some((off, l)) = line_at(&t, i) else { return Keep };
            let lw = l.to_lowercase();
            let Some(kw) = [lw.find("break"), lw.find("continue")].into_iter().flatten().min() else { return Keep };
            return if in_loop_body(t.as_bytes(), off + kw) { Drop } else { Keep };
        }
        if f.rule_id.contains(".RemovedFunctions.") {
            let Some(name) = rule_ident(&f.rule_id) else { return Keep };
            let t = text(&f.file);
            if imports_function(&t, &name) {
                return Drop;
            }
            let Some(ns) = ns_re.captures(&t).map(|c| c[1].to_lowercase()) else { return Keep };
            let decls = ns_functions.get_or_insert_with(|| namespaced_functions(root, &ns_re, &fn_re));
            return if decls.get(&ns).map_or(false, |s| s.contains(&name)) { Drop } else { Keep };
        }
        if f.rule_id.contains("ForbiddenThisUseContexts.Global") {
            let t = text(&f.file);
            let Some((_, l)) = line_at(&t, i) else { return Keep };
            if let Some(c) = global_re.captures(l) {
                let names = dyn_var.replace_all(&c[1], "");
                if c[1].contains("${") && !this_re.is_match(&names) {
                    return Drop;
                }
            }
            return Keep;
        }
        if f.rule_id.contains("InternalInterfaces.datetimeinterfaceFound") {
            let t = text(&f.file);
            let lines: Vec<&str> = t.lines().collect();
            let Some(l) = lines.get(i) else { return Keep };
            if iface_re.is_match(l) {
                return Drop;
            }
            // Multi-line declaration: the `extends` line has no class/interface keyword itself.
            if !decl_re.is_match(l) {
                let from = i.saturating_sub(2);
                let head = lines[from..=i].join(" ");
                if iface_re.is_match(&head) && !Regex::new(r"(?i)\bclass\s").unwrap().is_match(&head) {
                    return Drop;
                }
            }
            return Keep;
        }
        if f.rule_id.contains("define_case_insensitive") {
            let t = text(&f.file);
            let Some((_, l)) = line_at(&t, i) else { return Keep };
            match define_third_arg(l).map(|a| a.to_lowercase()) {
                Some(a) if a == "false" || a == "\\false" => return Drop,
                Some(a) if a.starts_with('$') => return Info("the third argument is passed through from a variable; PHP 8 only rejects the value true"),
                _ => {}
            }
            return Keep;
        }
        if f.rule_id.contains("InternalInterfaces.throwableFound") {
            // `class X extends SomeException implements Throwable` is valid PHP; only a class that
            // extends nothing fails, and that fails on every PHP 7+ version.
            let t = text(&f.file);
            let lines: Vec<&str> = t.lines().collect();
            if i >= lines.len() {
                return Keep;
            }
            let head = lines[i.saturating_sub(3)..=(i + 2).min(lines.len() - 1)].join(" ");
            return if throwable_sub.is_match(&head) { Drop } else { Keep };
        }
        if f.rule_id.contains("RemovedImplodeFlexibleParamOrder") {
            // `implode(Foo::SEP, [ ... ])`: the pieces are an array literal in second place, so the
            // order is right; the rule cannot tell the type of a class constant.
            let t = text(&f.file);
            let Some((off, l)) = line_at(&t, i) else { return Keep };
            let Some(m) = implode_call.find(l) else { return Keep };
            let args = call_args(&t[off + m.end()..], 2);
            let is_array = |a: &str| {
                let a = a.trim_start().to_lowercase();
                a.starts_with('[') || Regex::new(r"^array\s*\(").unwrap().is_match(&a)
            };
            return match args.as_slice() {
                [a, b] if !is_array(a) && is_array(b) => Drop,
                _ => Keep,
            };
        }
        if f.rule_id.contains("NewLateStaticBinding.OutsideClassScope") {
            let t = text(&f.file);
            if !type_decl.is_match(&t) {
                return Info("this file declares no class, so it is a template included from a class method, where static:: is valid");
            }
            return Keep;
        }
        if f.rule_id.contains("Classes.RemovedOrphanedParent") {
            let t = text(&f.file);
            let Some((_, l)) = line_at(&t, i) else { return Keep };
            if !parent_call.is_match(l) {
                return Info("a bare word parent (for example parent.isObject()), not a parent:: call; this existing bug is not changed by the upgrade");
            }
            return Keep;
        }
        if f.rule_id.contains("RemovedTernaryAssociativity") {
            let t = text(&f.file);
            if let Some((off, _)) = line_at(&t, i) {
                if in_match_block(t.as_bytes(), off) {
                    return Info("inside a match expression, so this file already compiles only on PHP 8, where an unparenthesised nested ternary is a compile error; the rule misreads parenthesised ternaries here");
                }
            }
        }
        Keep
    };
    let decisions: Vec<Misread> = findings.iter().map(&mut decide).collect();
    let mut it = decisions.into_iter();
    findings.retain_mut(|f| match it.next() {
        Some(Misread::Drop) => false,
        Some(Misread::Info(why)) => {
            if f.severity == "error" || f.severity == "warning" {
                f.severity = "info".into();
                f.confidence = "low".into();
                f.message = format!("{} ({why})", f.message);
            }
            true
        }
        _ => true,
    });
}

enum Misread {
    Keep,
    Drop,
    Info(&'static str),
}

/// Byte offset and text of 0-based line `i`.
fn line_at(t: &str, i: usize) -> Option<(usize, &str)> {
    let mut off = 0;
    for (k, l) in t.split('\n').enumerate() {
        if k == i {
            return Some((off, l));
        }
        off += l.len() + 1;
    }
    None
}

/// `use function A\B\name;` (or a group import containing `name`) without an `as` alias.
fn imports_function(text: &str, name: &str) -> bool {
    Regex::new(&format!(r"(?i)\buse\s+function\s+[^;]*[\\{{,]\s*{}\s*[,;}}]", regex::escape(name))).unwrap().is_match(text)
}

/// The first `max` arguments of a call, from the text just after its opening `(`, trimmed. The
/// text may span lines. Fewer are returned when the call ends first or the text is cut off.
fn call_args(text: &str, max: usize) -> Vec<String> {
    let mut args = vec![String::new()];
    let (mut depth, mut quote) = (0i32, None::<char>);
    let mut prev = ' ';
    for ch in text.chars().take(20_000) {
        match quote {
            Some(q) => {
                if ch == q && prev != '\\' {
                    quote = None;
                }
                args.last_mut().unwrap().push(ch);
            }
            None => match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    args.last_mut().unwrap().push(ch);
                }
                '(' | '[' | '{' => {
                    depth += 1;
                    args.last_mut().unwrap().push(ch);
                }
                ')' | ']' | '}' if depth == 0 => {
                    args.truncate(max);
                    return args.into_iter().map(|a| a.trim().to_string()).collect();
                }
                ')' | ']' | '}' => {
                    depth -= 1;
                    args.last_mut().unwrap().push(ch);
                }
                ',' if depth == 0 => {
                    if args.len() == max {
                        break;
                    }
                    args.push(String::new());
                }
                _ => args.last_mut().unwrap().push(ch),
            },
        }
        prev = if prev == '\\' && ch == '\\' { ' ' } else { ch };
    }
    args.truncate(max);
    args.into_iter().map(|a| a.trim().to_string()).collect()
}

/// Third argument of the first `define(` call on the line, trimmed.
fn define_third_arg(line: &str) -> Option<String> {
    let m = Regex::new(r"(?i)\bdefine\s*\(").unwrap().find(line)?;
    let mut args = vec![String::new()];
    let (mut depth, mut quote) = (0i32, None::<char>);
    let mut prev = ' ';
    for ch in line[m.end()..].chars() {
        match quote {
            Some(q) => {
                if ch == q && prev != '\\' {
                    quote = None;
                }
                args.last_mut()?.push(ch);
            }
            None => match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    args.last_mut()?.push(ch);
                }
                '(' | '[' => {
                    depth += 1;
                    args.last_mut()?.push(ch);
                }
                ')' | ']' if depth == 0 => break,
                ')' | ']' => {
                    depth -= 1;
                    args.last_mut()?.push(ch);
                }
                ',' if depth == 0 => args.push(String::new()),
                _ => args.last_mut()?.push(ch),
            },
        }
        prev = ch;
    }
    args.get(2).map(|a| a.trim().to_string())
}

fn skip_ws_back(t: &[u8], mut i: usize) -> usize {
    while i > 0 && t[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    i
}

/// The word ending at `end` (exclusive), lower-case, and its start.
fn word_before(t: &[u8], end: usize) -> (usize, String) {
    let mut s = end;
    while s > 0 && (t[s - 1].is_ascii_alphanumeric() || t[s - 1] == b'_') {
        s -= 1;
    }
    (s, String::from_utf8_lossy(&t[s..end]).to_lowercase())
}

/// Index of the opening bracket matching the closing one at `close`.
fn match_back(t: &[u8], close: usize, open_c: u8, close_c: u8) -> Option<usize> {
    let mut depth = 0i64;
    let mut i = close + 1;
    let floor = close.saturating_sub(20_000);
    while i > floor {
        i -= 1;
        if t[i] == close_c {
            depth += 1;
        } else if t[i] == open_c {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Start of the `if` that begins the chain ending in the `else`/`elseif`/`catch`/`finally`
/// keyword at `kw` (or of the `try` for `catch`/`finally`).
fn chain_start(t: &[u8], mut kw: usize) -> Option<usize> {
    for _ in 0..64 {
        let e = skip_ws_back(t, kw);
        if e == 0 || t[e - 1] != b'}' {
            return None; // braceless body before `else`: not followed
        }
        let o = match_back(t, e - 1, b'{', b'}')?;
        let e2 = skip_ws_back(t, o);
        if e2 == 0 {
            return None;
        }
        let (ws, w) = if t[e2 - 1] == b')' { word_before(t, skip_ws_back(t, match_back(t, e2 - 1, b'(', b')')?)) } else { word_before(t, e2) };
        match w.as_str() {
            "if" | "try" => return Some(ws),
            "elseif" | "catch" | "else" | "finally" => kw = ws,
            _ => return None,
        }
    }
    None
}

/// True when the statement starting at byte `pos` (a `break`/`continue`) is inside a loop
/// (or `switch`), following braceless loop bodies and nested braceless heads backwards.
fn in_loop_body(t: &[u8], mut pos: usize) -> bool {
    for _ in 0..256 {
        let e = skip_ws_back(t, pos);
        if e == 0 {
            return false;
        }
        match t[e - 1] {
            b')' => {
                let Some(o) = match_back(t, e - 1, b'(', b')') else { return false };
                let (ws, w) = word_before(t, skip_ws_back(t, o));
                match w.as_str() {
                    "for" | "foreach" | "while" => return true,
                    "if" => pos = ws,
                    "elseif" => match chain_start(t, ws) {
                        Some(p) => pos = p,
                        None => return false,
                    },
                    _ => return false,
                }
            }
            b'{' => {
                let e2 = skip_ws_back(t, e - 1);
                if e2 == 0 {
                    return false;
                }
                if t[e2 - 1] == b')' {
                    let Some(o) = match_back(t, e2 - 1, b'(', b')') else { return false };
                    let (ws, w) = word_before(t, skip_ws_back(t, o));
                    match w.as_str() {
                        "for" | "foreach" | "while" | "switch" => return true,
                        "if" => pos = ws,
                        "elseif" | "catch" => match chain_start(t, ws) {
                            Some(p) => pos = p,
                            None => return false,
                        },
                        _ => return false,
                    }
                } else {
                    let (ws, w) = word_before(t, e2);
                    match w.as_str() {
                        "do" => return true,
                        "try" => pos = ws,
                        "else" | "finally" => match chain_start(t, ws) {
                            Some(p) => pos = p,
                            None => return false,
                        },
                        _ => return false,
                    }
                }
            }
            b';' | b'}' | b':' => {
                // A sibling statement: move out to the enclosing block's `{`.
                let mut depth = 0i64;
                let mut i = e;
                let floor = e.saturating_sub(20_000);
                let mut found = None;
                while i > floor {
                    i -= 1;
                    match t[i] {
                        b'}' => depth += 1,
                        b'{' if depth == 0 => {
                            found = Some(i);
                            break;
                        }
                        b'{' => depth -= 1,
                        _ => {}
                    }
                }
                match found {
                    Some(o) => pos = o + 1,
                    None => return false,
                }
            }
            _ => {
                let (ws, w) = word_before(t, e);
                match w.as_str() {
                    "do" => return true,
                    "try" => pos = ws,
                    "else" => match chain_start(t, ws) {
                        Some(p) => pos = p,
                        None => return false,
                    },
                    _ => return false,
                }
            }
        }
    }
    false
}

/// True when byte `pos` lies inside the `{ }` body of a `match (...)` expression.
fn in_match_block(t: &[u8], pos: usize) -> bool {
    let mut depth = 0i64;
    let mut i = pos;
    let floor = pos.saturating_sub(20_000);
    while i > floor {
        i -= 1;
        match t[i] {
            b'}' => depth += 1,
            b'{' if depth > 0 => depth -= 1,
            b'{' => {
                let e = skip_ws_back(t, i);
                if e > 0 && t[e - 1] == b')' {
                    if let Some(o) = match_back(t, e - 1, b'(', b')') {
                        let (ws, w) = word_before(t, skip_ws_back(t, o));
                        let before = skip_ws_back(t, ws);
                        let member = before >= 2 && (&t[before - 2..before] == b"->" || &t[before - 2..before] == b"::");
                        if w == "match" && !member {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
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
    let version_guard = Regex::new(&format!("(?i){VERSION_GUARD}")).unwrap();
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
                    let Some(dir) = version_dir(&line[v.start()..], MIN_TARGET_ID) else { return false };
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
    apply_package_context(root, findings);
    apply_wp_polyfills(root, rules_dir, findings);
    pair_extension_findings(findings);
}

/// Composer package names whose signature rule uses a different name.
const COMPONENT_ALIASES: &[(&str, &str)] = &[("google/apiclient", "google-api-client")];

/// Name a component is known by across detectors: `guzzlehttp/guzzle` -> `guzzle`,
/// `google/apiclient` -> `google-api-client`.
fn component_key(name: &str) -> String {
    let lower = name.to_lowercase();
    if let Some((_, alias)) = COMPONENT_ALIASES.iter().find(|(c, _)| *c == lower) {
        return alias.to_string();
    }
    lower.rsplit('/').next().unwrap_or(&lower).to_string()
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
