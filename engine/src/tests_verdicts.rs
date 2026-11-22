//! Unit and golden tests for the verdict logic: guards, fallbacks, suppressions, component
//! attribution, hidden-library and requirement detection, and the verdict rules in `store()`.
//!
//! Everything here is offline: findings are built directly (no phpcs), PHP fixtures are tiny
//! hand-written snippets written to unique temp directories, and rules come from `engine/rules`.

use crate::detect;
use crate::model::{Component, Finding};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------- helpers ----------

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A temp directory with PHP files, removed when dropped.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("leapcheck-test-{}-{}-{}", std::process::id(), name, n));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn file(self, rel: &str, content: &str) -> Self {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
        self
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn rules_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("rules")
}

fn rules() -> detect::Rules {
    detect::Rules::load(&rules_dir()).expect("engine/rules must load")
}

fn finding(rule_id: &str, severity: &str, file: &str, line: u32) -> Finding {
    Finding {
        rule_id: rule_id.into(),
        severity: severity.into(),
        confidence: if severity == "error" { "high".into() } else { "medium".into() },
        message: "test message".into(),
        file: file.into(),
        line,
        component: None,
        analyzer: "php:phpcompatibility".into(),
    }
}

/// 1-based line number of the first line containing `needle`.
fn line_of(src: &str, needle: &str) -> u32 {
    src.lines().position(|l| l.contains(needle)).expect("needle not in fixture") as u32 + 1
}

/// The same post-processing pipeline the scanner runs on analyzer output.
fn pipeline(root: &Path, rules_dir: &Path, findings: &mut Vec<Finding>, comps: &[Component]) {
    for f in findings.iter_mut() {
        f.component = detect::attribute(&f.file, comps);
    }
    detect::apply_suppressions(rules_dir, findings);
    detect::apply_guards(root, findings);
    detect::apply_fallbacks(root, findings);
}

const MYSQL_QUERY: &str = "PHPCompatibility.FunctionUse.RemovedFunctions.mysql_queryDeprecatedRemoved";
const MYSQL_EXT: &str = "PHPCompatibility.Extensions.RemovedExtensions.mysql_DeprecatedRemoved";
const MCRYPT_FN: &str = "PHPCompatibility.FunctionUse.RemovedFunctions.mcrypt_encryptDeprecatedRemoved";
const MCRYPT_EXT: &str = "PHPCompatibility.Extensions.RemovedExtensions.mcryptDeprecatedRemoved";
const CREATE_FUNCTION: &str = "PHPCompatibility.FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved";
const EACH: &str = "PHPCompatibility.FunctionUse.RemovedFunctions.eachDeprecatedRemoved";
const CARBON_RULE: &str = "PHPCompatibility.Interfaces.InternalInterfaces.datetimeinterfaceFound";
const ARGS_CHANGED: &str = "PHPCompatibility.FunctionUse.ArgumentFunctionsReportCurrentValue.Changed";

// ---------- guards ----------

#[test]
fn error_inside_function_exists_guard_becomes_info() {
    let src = "<?php\n\
               function enc($d, $k) {\n\
               \tif (function_exists('mcrypt_encrypt')) {\n\
               \t\treturn mcrypt_encrypt(MCRYPT_RIJNDAEL_128, $k, $d, MCRYPT_MODE_ECB);\n\
               \t}\n\
               \treturn openssl_encrypt($d, 'aes-128-ecb', $k);\n\
               }\n";
    let fx = Fixture::new("guard").file("inc/crypto.php", src);
    let line = line_of(src, "return mcrypt_encrypt");
    let mut fs = vec![finding(MCRYPT_FN, "error", "inc/crypto.php", line), finding(MCRYPT_EXT, "error", "inc/crypto.php", line)];
    detect::apply_guards(fx.path(), &mut fs);
    for f in &fs {
        assert_eq!(f.severity, "info", "{}", f.rule_id);
        assert_eq!(f.confidence, "low");
        assert!(f.message.starts_with("Guarded"), "{}", f.message);
    }
}

#[test]
fn error_after_php_version_check_becomes_info() {
    let src = "<?php\n\
               if (PHP_VERSION_ID < 70200) {\n\
               \twhile (list($k, $v) = each($arr)) { echo $k; }\n\
               }\n";
    let fx = Fixture::new("verguard").file("legacy.php", src);
    let mut fs = vec![finding(EACH, "error", "legacy.php", line_of(src, "each("))];
    detect::apply_guards(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
}

#[test]
fn unguarded_removed_function_stays_error() {
    let src = "<?php\n\
               $cb = create_function('$a', 'return $a * 2;');\n\
               $r = mysql_query('SELECT 1');\n";
    let fx = Fixture::new("unguarded").file("main.php", src);
    let mut fs = vec![
        finding(CREATE_FUNCTION, "error", "main.php", line_of(src, "create_function")),
        finding(MYSQL_QUERY, "error", "main.php", line_of(src, "mysql_query")),
    ];
    pipeline(fx.path(), &rules_dir(), &mut fs, &[]);
    for f in &fs {
        assert_eq!(f.severity, "error", "{}", f.rule_id);
        assert_eq!(f.confidence, "high");
        assert_eq!(f.message, "test message");
    }
}

#[test]
fn guard_for_a_different_function_does_not_apply() {
    let src = "<?php\n\
               if (function_exists('mb_strlen')) { $n = mb_strlen($s); }\n\
               $cb = create_function('$a', 'return $a;');\n";
    let fx = Fixture::new("otherguard").file("x.php", src);
    let mut fs = vec![finding(CREATE_FUNCTION, "error", "x.php", line_of(src, "create_function"))];
    detect::apply_guards(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
}

#[test]
fn guard_more_than_80_lines_above_does_not_apply() {
    let mut src = String::from("<?php\nif (function_exists('create_function')) { $x = 1; }\n");
    for _ in 0..90 {
        src.push_str("$y = 1;\n");
    }
    src.push_str("$cb = create_function('$a', 'return $a;');\n");
    let fx = Fixture::new("farguard").file("far.php", &src);
    let mut fs = vec![finding(CREATE_FUNCTION, "error", "far.php", line_of(&src, "$cb = create_function"))];
    detect::apply_guards(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
}

#[test]
fn guards_ignore_warnings_and_line_zero() {
    let src = "<?php\nif (function_exists('mcrypt_encrypt')) { mcrypt_encrypt(1,2,3,4); }\n";
    let fx = Fixture::new("guardskip").file("a.php", src);
    let mut fs = vec![finding(MCRYPT_FN, "warning", "a.php", 2), finding(MCRYPT_FN, "error", "a.php", 0)];
    detect::apply_guards(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "warning");
    assert_eq!(fs[1].severity, "error");
}

// ---------- fallbacks ----------

#[test]
fn mysqli_else_mysql_in_same_file_becomes_info() {
    let src = "<?php\n\
               function db_q($sql) {\n\
               \tglobal $is_mysqli, $link;\n\
               \tif ($is_mysqli) {\n\
               \t\treturn mysqli_query($link, $sql);\n\
               \t} else {\n\
               \t\treturn mysql_query($sql, $link);\n\
               \t}\n\
               }\n";
    let fx = Fixture::new("fallback-same").file("db.php", src);
    let line = line_of(src, "mysql_query(");
    let mut fs = vec![finding(MYSQL_QUERY, "error", "db.php", line), finding(MYSQL_EXT, "error", "db.php", line)];
    pipeline(fx.path(), &rules_dir(), &mut fs, &[]);
    for f in &fs {
        assert_eq!(f.severity, "info", "{}", f.rule_id);
        assert_eq!(f.confidence, "low");
        assert!(f.message.starts_with("Fallback code"), "{}", f.message);
    }
}

#[test]
fn mysql_without_alternative_in_file_but_mysqli_elsewhere_is_low_confidence_warning() {
    let legacy = "<?php\n$r = mysql_query('SELECT 1');\n";
    let modern = "<?php\n$r = mysqli_query($link, 'SELECT 1');\n";
    let fx = Fixture::new("fallback-pkg").file("legacy/old-db.php", legacy).file("inc/db.php", modern);
    let mut fs = vec![finding(MYSQL_QUERY, "error", "legacy/old-db.php", 2)];
    pipeline(fx.path(), &rules_dir(), &mut fs, &[]);
    assert_eq!(fs[0].severity, "warning");
    assert_eq!(fs[0].confidence, "low");
    assert!(fs[0].message.starts_with("Likely fallback code"), "{}", fs[0].message);
}

#[test]
fn mcrypt_with_openssl_in_same_file_becomes_info() {
    let src = "<?php\n$a = openssl_encrypt($d, 'aes-256-cbc', $k);\n$b = mcrypt_encrypt(MCRYPT_RIJNDAEL_128, $k, $d, 'cbc');\n";
    let fx = Fixture::new("fallback-mcrypt").file("c.php", src);
    let mut fs = vec![finding(MCRYPT_FN, "error", "c.php", 3)];
    detect::apply_fallbacks(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
}

#[test]
fn fallbacks_do_not_touch_rules_outside_the_fallback_table() {
    let src = "<?php\n$x = mysqli_query($l, 'x');\n$cb = create_function('$a', 'return $a;');\n";
    let fx = Fixture::new("fallback-other").file("m.php", src);
    let mut fs = vec![finding(CREATE_FUNCTION, "error", "m.php", 3)];
    detect::apply_fallbacks(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
}

// ---------- suppressions (real engine/rules/suppressions.json) ----------

#[test]
fn suppression_removes_carbon_datetimeinterface_case_only() {
    let mut fs = vec![
        finding(CARBON_RULE, "error", "vendor/nesbot/carbon/src/Carbon/CarbonInterface.php", 12),
        finding(CARBON_RULE, "error", "includes/class-my-date.php", 5),
        finding(CREATE_FUNCTION, "error", "vendor/nesbot/carbon/src/Carbon/CarbonInterface.php", 40),
    ];
    detect::apply_suppressions(&rules_dir(), &mut fs);
    assert_eq!(fs.len(), 2);
    assert!(fs.iter().all(|f| !(f.rule_id == CARBON_RULE && f.file.ends_with("CarbonInterface.php"))));
    // Other DateTimeInterface findings are an existing PHP 5.5 restriction, not an upgrade change (N6).
    assert!(fs.iter().any(|f| f.rule_id == CARBON_RULE && f.file == "includes/class-my-date.php" && f.severity == "info"));
    assert!(fs.iter().any(|f| f.rule_id == CREATE_FUNCTION && f.severity == "error"));
}

#[test]
fn suppression_warning_action_downgrades_error_and_adds_reason() {
    // A deprecated (not removed) ini directive keeps the "warning" action.
    let ini = "PHPCompatibility.IniDirectives.RemovedIniDirectives.auto_detect_line_endingsDeprecated";
    let mut fs = vec![finding(ini, "error", "inc/util.php", 9), finding(ini, "warning", "inc/util.php", 20)];
    detect::apply_suppressions(&rules_dir(), &mut fs);
    assert_eq!(fs.len(), 2, "warning action must not remove findings");
    assert_eq!(fs[0].severity, "warning");
    assert!(fs[0].message.starts_with("test message ("), "{}", fs[0].message);
    assert!(fs[0].message.contains("ini_set()/ini_get()"), "{}", fs[0].message);
    assert_eq!(fs[1].message, "test message", "existing warnings are left unchanged");
}

#[test]
fn suppressions_with_custom_rules_file() {
    let fx = Fixture::new("supp").file(
        "suppressions.json",
        r#"[{"rule_id":"R.remove","path_regex":"^lib/"},{"rule_id":"R.warn","path_regex":".*","action":"warning","reason":"why"},{"rule_id":"R.bad","path_regex":"("}]"#,
    );
    let mut fs = vec![
        finding("R.remove", "error", "lib/a.php", 1),
        finding("R.remove", "error", "src/a.php", 1),
        finding("R.warn", "error", "src/a.php", 2),
        finding("R.bad", "error", "src/a.php", 3),
    ];
    detect::apply_suppressions(fx.path(), &mut fs);
    let got: Vec<(&str, &str, &str)> = fs.iter().map(|f| (f.rule_id.as_str(), f.file.as_str(), f.severity.as_str())).collect();
    assert_eq!(got, vec![("R.remove", "src/a.php", "error"), ("R.warn", "src/a.php", "warning"), ("R.bad", "src/a.php", "error")]);
    assert_eq!(fs[1].message, "test message (why)");
}

#[test]
fn missing_suppressions_file_is_a_no_op() {
    let fx = Fixture::new("nosupp");
    let mut fs = vec![finding(CARBON_RULE, "error", "CarbonInterface.php", 1)];
    detect::apply_suppressions(fx.path(), &mut fs);
    assert_eq!(fs.len(), 1);
}

// ---------- attribution and version ordering ----------

fn comp(name: &str, version: Option<&str>, path: &str) -> Component {
    Component {
        name: name.into(),
        version: version.map(String::from),
        kind: "php".into(),
        path: path.into(),
        detection: "signature".into(),
        known_vulnerabilities: 0,
    }
}

#[test]
fn attribute_picks_deepest_component_on_path_boundaries() {
    let comps = vec![
        comp("guzzlehttp/guzzle", Some("7.8.1"), "vendor/guzzlehttp/guzzle"),
        comp("vendor-root", None, "vendor"),
        comp("phpmailer", Some("5.2.7"), "lib/PHPMailer"),
        comp("root-lib", Some("1.0"), ""),
    ];
    assert_eq!(detect::attribute("vendor/guzzlehttp/guzzle/src/Client.php", &comps).as_deref(), Some("guzzlehttp/guzzle@7.8.1"));
    assert_eq!(detect::attribute("vendor/other/x.php", &comps).as_deref(), Some("vendor-root"));
    assert_eq!(detect::attribute("lib/PHPMailer/class.phpmailer.php", &comps).as_deref(), Some("phpmailer@5.2.7"));
    // "lib/PHPMailerExtra" shares a prefix but is not inside "lib/PHPMailer".
    assert_eq!(detect::attribute("lib/PHPMailerExtra/x.php", &comps), None);
    // Components at the package root (empty path) never claim files.
    assert_eq!(detect::attribute("main.php", &comps), None);
}

#[test]
fn ver_cmp_orders_versions_numerically() {
    use std::cmp::Ordering::*;
    assert_eq!(detect::ver_cmp("5.2.7", "5.2.10"), Less);
    assert_eq!(detect::ver_cmp("6.0", "5.99.99"), Greater);
    assert_eq!(detect::ver_cmp("1.0", "1.0.0"), Equal);
    assert_eq!(detect::ver_cmp("v2.1", "2.1"), Equal);
    assert_eq!(detect::ver_cmp("10.0.0", "9.9.9"), Greater);
    let mut v = vec!["1.10.0", "1.2.0", "1.9.3", "1.2"];
    v.sort_by(|a, b| detect::ver_cmp(a, b));
    assert_eq!(v, vec!["1.2.0", "1.2", "1.9.3", "1.10.0"]);
}

// ---------- hidden libraries and requirements (real engine/rules) ----------

#[test]
fn signature_rule_detects_phpmailer_versions() {
    let fx = Fixture::new("sig")
        .file("lib/PHPMailer/class.phpmailer.php", "<?php\nclass PHPMailer {\n    public $Version = '5.2.7';\n}\n")
        .file("vendor/phpmailer/phpmailer/src/PHPMailer.php", "<?php\nnamespace PHPMailer\\PHPMailer;\nclass PHPMailer {\n    const VERSION = '6.9.1';\n}\n")
        // Wrapper with a similar name must not be detected (case-sensitive rule).
        .file("inc/class-sendpress-phpmailer.php", "<?php\nclass SendPress_PHPMailer { public $Version = '1.0'; }\n");
    let comps = detect::components(fx.path(), &rules());
    let mailers: Vec<(&str, Option<&str>, &str)> =
        comps.iter().filter(|c| c.name == "phpmailer").map(|c| (c.path.as_str(), c.version.as_deref(), c.detection.as_str())).collect();
    assert_eq!(mailers, vec![("lib/PHPMailer", Some("5.2.7"), "signature"), ("vendor/phpmailer/phpmailer/src", Some("6.9.1"), "signature")]);
    assert_eq!(detect::attribute("lib/PHPMailer/class.smtp.php", &comps).as_deref(), Some("phpmailer@5.2.7"));
}

#[test]
fn composer_installed_json_components() {
    let fx = Fixture::new("composer").file(
        "vendor/composer/installed.json",
        r#"{"packages":[{"name":"nesbot/carbon","version":"v2.72.3"},{"name":"psr/log","version":"1.1.4"}]}"#,
    );
    let comps = detect::components(fx.path(), &rules());
    let got: Vec<(&str, Option<&str>, &str)> = comps.iter().map(|c| (c.name.as_str(), c.version.as_deref(), c.path.as_str())).collect();
    assert_eq!(got, vec![("nesbot/carbon", Some("2.72.3"), "vendor/nesbot/carbon"), ("psr/log", Some("1.1.4"), "vendor/psr/log")]);
}

#[test]
fn php_extension_pattern_hit_reports_file_and_line() {
    let src = "<?php\n\
               // Thumbnails\n\
               function thumb($w, $h) {\n\
               \t$im = imagecreatetruecolor($w, $h);\n\
               \treturn $im;\n\
               }\n";
    let fx = Fixture::new("ext").file("inc/image.php", src);
    let reqs = detect::requirements(fx.path(), &rules());
    let gd = reqs.iter().find(|r| r.name == "gd").expect("gd requirement");
    assert_eq!(gd.evidence_file, "inc/image.php");
    assert_eq!(gd.evidence_line, 4);
}

#[test]
fn comments_are_ignored_by_requirements() {
    let src = "<?php\n\
               // $im = imagecreatetruecolor(1, 1);\n\
               # $im = imagecreatetruecolor(1, 1);\n\
               /**\n\
               \x20* $im = imagecreatetruecolor(1, 1);\n\
               \x20*/\n\
               $x = 1;\n";
    let fx = Fixture::new("comments").file("c.php", src);
    let reqs = detect::requirements(fx.path(), &rules());
    assert!(reqs.iter().all(|r| r.name != "gd"), "{:?}", reqs.iter().map(|r| (&r.name, r.evidence_line)).collect::<Vec<_>>());
}

#[test]
fn requirements_only_read_php_files() {
    let fx = Fixture::new("nonphp").file("readme.txt", "$im = imagecreatetruecolor(1, 1);\n");
    assert!(detect::requirements(fx.path(), &rules()).is_empty());
}

// ---------- verdict rules (store) ----------

fn verdicts(per_target: &[(String, Vec<Finding>)], encoded: u32) -> Vec<(String, String, i64, i64)> {
    let fx = Fixture::new("db");
    let db = fx.path().join("t.sqlite");
    let conn = crate::db::open(db.to_str().unwrap()).unwrap();
    let run = crate::trace::Run::start(&conn, "test", serde_json::json!({})).unwrap();
    crate::store(&conn, &run.id, "wordpress-plugin", "fixture", "1.0", &[], &[], per_target, encoded, &|_| false).unwrap();
    let mut stmt = conn.prepare("SELECT target, verdict, errors, warnings FROM result ORDER BY target").unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

#[test]
fn verdict_rules() {
    let t = |name: &str, fs: Vec<Finding>| (name.to_string(), fs);
    let per_target = vec![
        t("8.0", vec![finding(EACH, "error", "a.php", 1), finding(EACH, "warning", "a.php", 2)]),
        t("8.1", vec![finding(EACH, "warning", "a.php", 2), finding(EACH, "info", "a.php", 3)]),
        t("8.2", vec![finding(EACH, "info", "a.php", 3)]),
        t("8.3", vec![]),
    ];
    let v = verdicts(&per_target, 0);
    let s = |a: &str, b: &str, e, w| (a.to_string(), b.to_string(), e, w);
    assert_eq!(
        v,
        vec![s("php:8.0", "breaks", 1, 1), s("php:8.1", "issues", 0, 1), s("php:8.2", "compatible", 0, 0), s("php:8.3", "compatible", 0, 0)]
    );
}

#[test]
fn encoded_files_never_yield_compatible() {
    let per_target = vec![
        ("8.3".to_string(), vec![]),
        ("8.4".to_string(), vec![finding(EACH, "warning", "a.php", 1)]),
        ("8.5".to_string(), vec![finding(EACH, "error", "a.php", 1)]),
    ];
    let v: Vec<(String, String)> = verdicts(&per_target, 2).into_iter().map(|(t, v, _, _)| (t, v)).collect();
    assert_eq!(
        v,
        vec![("php:8.3".into(), "unknown".into()), ("php:8.4".into(), "unknown".into()), ("php:8.5".into(), "breaks".into())]
    );
}

// ---------- golden: whole pipeline on a small multi-file fixture ----------

#[test]
fn golden_pipeline_fixture_plugin() {
    let main = "<?php\n\
                /* Plugin Name: Fixture */\n\
                require __DIR__ . '/inc/db.php';\n\
                $cb = create_function('$a', 'return $a;');\n";
    let db = "<?php\n\
              function q($sql) {\n\
              \tif (function_exists('mysqli_query')) {\n\
              \t\treturn mysqli_query($GLOBALS['l'], $sql);\n\
              \t}\n\
              \treturn mysql_query($sql);\n\
              }\n";
    let old = "<?php\n$r = mysql_query('SELECT 2');\n";
    let crypto = "<?php\nif (function_exists('mcrypt_encrypt')) {\n\t$c = mcrypt_encrypt(1, 2, 3, 4);\n}\n";
    let fx = Fixture::new("golden")
        .file("fixture.php", main)
        .file("inc/db.php", db)
        .file("legacy/old.php", old)
        .file("lib/crypto/crypto.php", crypto)
        .file("vendor/nesbot/carbon/src/Carbon/CarbonInterface.php", "<?php\ninterface CarbonInterface extends DateTimeInterface {}\n")
        .file("vendor/composer/installed.json", r#"[{"name":"nesbot/carbon","version":"2.72.3"}]"#);
    let comps = detect::components(fx.path(), &rules());
    let mut fs = vec![
        finding(CREATE_FUNCTION, "error", "fixture.php", 4),
        finding(MYSQL_QUERY, "error", "inc/db.php", 6),
        finding(MYSQL_QUERY, "error", "legacy/old.php", 2),
        finding(MCRYPT_FN, "error", "lib/crypto/crypto.php", 3),
        finding(CARBON_RULE, "error", "vendor/nesbot/carbon/src/Carbon/CarbonInterface.php", 2),
        finding(ARGS_CHANGED, "error", "vendor/nesbot/carbon/src/Carbon/Carbon.php", 10),
    ];
    pipeline(fx.path(), &rules_dir(), &mut fs, &comps);
    let got: Vec<String> = fs
        .iter()
        .map(|f| format!("{}:{} {} {}/{} {}", f.file, f.line, f.rule_id.rsplit('.').next().unwrap(), f.severity, f.confidence, f.component.as_deref().unwrap_or("-")))
        .collect();
    let expected = vec![
        "fixture.php:4 create_functionDeprecatedRemoved error/high -",
        "inc/db.php:6 mysql_queryDeprecatedRemoved info/low -",
        "legacy/old.php:2 mysql_queryDeprecatedRemoved warning/low -",
        "lib/crypto/crypto.php:3 mcrypt_encryptDeprecatedRemoved info/low -",
        "vendor/nesbot/carbon/src/Carbon/Carbon.php:10 Changed info/high nesbot/carbon@2.72.3",
    ];
    assert_eq!(got, expected, "\nactual:\n{}\n", got.join("\n"));
    let v = verdicts(&[("8.4".to_string(), fs)], 0);
    assert_eq!(v, vec![("php:8.4".to_string(), "breaks".to_string(), 1, 1)]);
}

// ---------- known limitations (documented, not yet fixed) ----------

#[test]

fn closed_guard_block_should_not_hide_later_unguarded_call() {
    let src = "<?php\n\
               if (function_exists('create_function')) {\n\
               \t$a = 1;\n\
               }\n\
               $cb = create_function('$a', 'return $a;');\n";
    let fx = Fixture::new("closedguard").file("g.php", src);
    let mut fs = vec![finding(CREATE_FUNCTION, "error", "g.php", 5)];
    detect::apply_guards(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
}

#[test]

fn negated_guard_should_not_downgrade() {
    let src = "<?php\n\
               if (!function_exists('mysql_query')) { die('needs mysql'); }\n\
               $r = mysql_query('SELECT 1');\n";
    let fx = Fixture::new("negguard").file("n.php", src);
    let mut fs = vec![finding(MYSQL_QUERY, "error", "n.php", 3)];
    detect::apply_guards(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
}

#[test]

fn alternative_in_comment_should_not_count_as_fallback() {
    let src = "<?php\n// TODO: port to mysqli\n$r = mysql_query('SELECT 1');\n";
    let fx = Fixture::new("commentalt").file("t.php", src);
    let mut fs = vec![finding(MYSQL_QUERY, "error", "t.php", 3)];
    detect::apply_fallbacks(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
}

#[test]

fn eregi_with_preg_alternative_should_be_fallback() {
    let src = "<?php\n$a = preg_match('/x/i', $s);\n$b = eregi('x', $s);\n";
    let fx = Fixture::new("eregi").file("e.php", src);
    let mut fs = vec![finding("PHPCompatibility.FunctionUse.RemovedFunctions.eregiDeprecatedRemoved", "error", "e.php", 3)];
    detect::apply_fallbacks(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
}

#[test]

fn trailing_comment_should_not_create_requirement() {
    let src = "<?php\n$x = 1; // was: $im = imagecreatetruecolor(1, 1);\n/* $im = imagecreatetruecolor(1, 1); */\n";
    let fx = Fixture::new("trailing").file("t.php", src);
    assert!(detect::requirements(fx.path(), &rules()).iter().all(|r| r.name != "gd"));
}

#[test]

fn ver_cmp_prerelease_sorts_before_release() {
    assert!(detect::ver_cmp("6.0.0-rc1", "6.0.0").is_lt());
    assert!(detect::ver_cmp("6.0.0-beta", "6.0.0").is_lt());
}

#[test]
fn guard_in_same_multiline_condition_becomes_info() {
    let src = "<?php\n\
               if (\n\
               \tdefined('MB_OVERLOAD_STRING')\n\
               \t\t&&\n\
               \t((int) ini_get('mbstring.func_overload')) & MB_OVERLOAD_STRING\n\
               ) {\n\
               \t$x = 1;\n\
               }\n";
    let fx = Fixture::new("sameexpr").file("lib/b.php", src);
    let line = line_of(src, "& MB_OVERLOAD_STRING");
    let mut fs = vec![finding("PHPCompatibility.Constants.RemovedConstants.mb_overload_stringRemoved", "error", "lib/b.php", line)];
    detect::apply_guards(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
}

#[test]
fn guard_statement_that_ended_does_not_cover_later_line() {
    let src = "<?php\n\
               $ok = function_exists('mysql_query');\n\
               mysql_query('SELECT 1');\n";
    let fx = Fixture::new("ended").file("a.php", src);
    let line = line_of(src, "mysql_query('SELECT");
    let mut fs = vec![finding("PHPCompatibility.FunctionUse.RemovedFunctions.mysql_queryDeprecatedRemoved", "error", "a.php", line)];
    detect::apply_guards(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
}

#[test]
fn new_constant_with_feature_detection_elsewhere_in_file_becomes_info() {
    let src = "<?php\n\
               function pick() {\n\
               \tswitch (true) {\n\
               \t\tcase defined('OPENSSL_PKCS1_PSS_PADDING'):\n\
               \t\t\treturn 'openssl';\n\
               \t}\n\
               }\n\
               function sign($m) {\n\
               \t$a = 1;\n\
               \treturn openssl_sign($m, $s, $k, 'sha256', OPENSSL_PKCS1_PSS_PADDING);\n\
               }\n";
    let fx = Fixture::new("newconst").file("RSA.php", src);
    let line = line_of(src, "openssl_sign(");
    let mut fs = vec![finding("PHPCompatibility.Constants.NewConstants.openssl_pkcs1_pss_paddingFound", "error", "RSA.php", line)];
    detect::apply_guards(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    // Without any feature detection the same use stays an error.
    let fx2 = Fixture::new("newconst2").file("RSA.php", &src.replace("case defined('OPENSSL_PKCS1_PSS_PADDING'):", "case 1:"));
    let mut fs2 = vec![finding("PHPCompatibility.Constants.NewConstants.openssl_pkcs1_pss_paddingFound", "error", "RSA.php", line)];
    detect::apply_guards(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "error");
}

#[test]
fn new_php_version_branch_is_not_a_guard() {
    let src = "<?php\nif (PHP_VERSION_ID >= 80000) {\n\t$f = create_function('$a', 'return $a;');\n}\n";
    let fx = Fixture::new("verdir").file("a.php", src);
    let line = line_of(src, "create_function");
    let mut fs = vec![finding("PHPCompatibility.FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "a.php", line)];
    detect::apply_guards(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
    // The old-PHP branch is a guard.
    let src2 = src.replace(">= 80000", "< 70200");
    let fx2 = Fixture::new("verdir2").file("a.php", &src2);
    let mut fs2 = vec![finding("PHPCompatibility.FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "a.php", line)];
    detect::apply_guards(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "info");
    // version_compare(..., '>=') selects new PHP: not a guard; '<' is.
    for (op, want) in [(">=", "error"), ("<", "info"), ("lt", "info")] {
        let s3 = format!("<?php\nif (version_compare(PHP_VERSION, '7.2', '{op}')) {{\n\t$f = create_function('$a', 'return $a;');\n}}\n");
        let fx3 = Fixture::new(&format!("vc{op}")).file("a.php", &s3);
        let mut fs3 = vec![finding("PHPCompatibility.FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "a.php", 3)];
        detect::apply_guards(fx3.path(), &mut fs3);
        assert_eq!(fs3[0].severity, want, "op {op}");
    }
}

// ---------- accuracy fixtures (false-positive classes found in the live-results review) ----------
//
// Each test is one fixture: the PHP code, the findings phpcs reports for it (rule IDs and lines as
// produced by the real analyzer), and the expected outcome after the full post-processing pipeline.

const P: &str = "PHPCompatibility.";

/// Full scan post-processing, exactly as `scan_root` runs it.
fn full(root: &Path, findings: &mut Vec<Finding>) {
    detect::post_process(root, &rules_dir(), findings);
}

fn verdict_of(fs: &[Finding]) -> String {
    verdicts(&[("8.4".to_string(), fs.to_vec())], 0).remove(0).1
}

fn sev(fs: &[Finding]) -> Vec<(String, String)> {
    fs.iter().map(|f| (f.rule_id.trim_start_matches(P).to_string(), f.severity.clone())).collect()
}

fn pf(rule: &str, severity: &str, file: &str, line: u32) -> Finding {
    finding(&format!("{P}{rule}"), severity, file, line)
}

#[test]
fn f1_extension_prefix_matches_are_dropped() {
    let src = "<?php\nfunction f1( $post ) {\n\t$d = mysql_to_rfc3339( $post->post_date );\n\t$t = \\MeprUtils::mysql_lifetime();\n\tfamne_print_checkbox( 'x' );\n\treturn array( $d, $t );\n}\n";
    let fx = Fixture::new("f1").file("f1.php", src);
    let mut fs = vec![
        pf("Extensions.RemovedExtensions.mysql_DeprecatedRemoved", "error", "f1.php", 3),
        pf("Extensions.RemovedExtensions.mysql_DeprecatedRemoved", "error", "f1.php", 4),
        pf("Extensions.RemovedExtensions.famRemoved", "error", "f1.php", 5),
    ];
    full(fx.path(), &mut fs);
    assert!(fs.is_empty(), "{:?}", sev(&fs));
    assert_eq!(verdict_of(&fs), "compatible");
}

#[test]
fn f1b_real_mysql_call_still_breaks_once() {
    let src = "<?php\nfunction f1b() {\n\treturn mysql_query( 'SELECT 1' );\n}\n";
    let fx = Fixture::new("f1b").file("f1b.php", src);
    let mut fs = vec![
        pf("Extensions.RemovedExtensions.mysql_DeprecatedRemoved", "error", "f1b.php", 3),
        pf("FunctionUse.RemovedFunctions.mysql_queryDeprecatedRemoved", "error", "f1b.php", 3),
    ];
    full(fx.path(), &mut fs);
    assert_eq!(fs.iter().filter(|f| f.severity == "error").count(), 1, "{:?}", sev(&fs));
    assert!(fs.iter().any(|f| f.rule_id.ends_with("mysql_queryDeprecatedRemoved") && f.severity == "error"));
    assert_eq!(verdict_of(&fs), "breaks");
}

#[test]
fn f2_wordpress_polyfill_depends_on_required_wp_version() {
    for (req, want_sev, want_verdict) in [("6.9", "info", "compatible"), ("6.0", "warning", "issues"), ("7.0", "info", "compatible")] {
        let src = format!("<?php\n/**\n * Plugin Name: F2\n * Requires at least: {req}\n */\nfunction f2( array $a ) {{\n\treturn array_first( $a );\n}}\n");
        let fx = Fixture::new("f2").file("f2.php", &src);
        let mut fs = vec![pf("FunctionUse.NewFunctions.array_firstFound", "error", "f2.php", 7)];
        full(fx.path(), &mut fs);
        assert_eq!(fs[0].severity, want_sev, "requires {req}");
        assert_eq!(verdict_of(&fs), want_verdict, "requires {req}");
    }
    // No header at all, but readme.txt says 6.9.
    let fx = Fixture::new("f2r").file("f2.php", "<?php\nreturn array_first( $a );\n").file("readme.txt", "=== F2 ===\nRequires at least: 6.9\n");
    assert_eq!(detect::wp_requires_at_least(fx.path()).as_deref(), Some("6.9"));
    // A function WordPress does not polyfill stays an error.
    let fx = Fixture::new("f2n").file("f2.php", "<?php\n/* Plugin Name: X\n * Requires at least: 6.9 */\nreturn mb_trim( $a );\n");
    let mut fs = vec![pf("FunctionUse.NewFunctions.mb_trimFound", "error", "f2.php", 4)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
}

#[test]
fn f3_early_return_negated_guard_protects_rest_of_function() {
    let src = "<?php\nclass F3 {\n\tpublic $s = -1;\n\tpublic function off() {\n\t\tif ( ! function_exists( 'get_magic_quotes_runtime' ) ) {\n\t\t\treturn 1;\n\t\t}\n\t\t$this->s = @get_magic_quotes_runtime();\n\t\treturn 1;\n\t}\n}\n";
    let fx = Fixture::new("f3").file("f3.php", src);
    let rule = "FunctionUse.RemovedFunctions.get_magic_quotes_runtimeDeprecatedRemoved";
    let mut fs = vec![pf(rule, "error", "f3.php", 8)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    assert_eq!(verdict_of(&fs), "compatible");
    // Braceless early return too.
    let src2 = "<?php\nfunction g() {\n\tif ( ! function_exists( 'get_magic_quotes_runtime' ) ) return;\n\t$s = get_magic_quotes_runtime();\n}\n";
    let fx2 = Fixture::new("f3b").file("f3.php", src2);
    let mut fs2 = vec![pf(rule, "error", "f3.php", 4)];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "info");
}

#[test]
fn early_exit_guard_limits() {
    let rule = "FunctionUse.RemovedFunctions.get_magic_quotes_runtimeDeprecatedRemoved";
    // die()/throw is not an early return: the code fails on new PHP.
    let die = "<?php\nfunction g() {\n\tif ( ! function_exists( 'get_magic_quotes_runtime' ) ) {\n\t\tdie( 'old PHP needed' );\n\t}\n\t$s = get_magic_quotes_runtime();\n}\n";
    // The early return sits in a different function that has closed.
    let other_fn = "<?php\nfunction a() {\n\tif ( ! function_exists( 'get_magic_quotes_runtime' ) ) {\n\t\treturn;\n\t}\n}\nfunction b() {\n\t$s = get_magic_quotes_runtime();\n}\n";
    // A block that does more than return does not end the function.
    let not_only_return = "<?php\nfunction g() {\n\tif ( ! function_exists( 'get_magic_quotes_runtime' ) ) {\n\t\t$x = 1;\n\t}\n\t$s = get_magic_quotes_runtime();\n}\n";
    for (name, src) in [("die", die), ("otherfn", other_fn), ("notonly", not_only_return)] {
        let fx = Fixture::new(name).file("g.php", src);
        let line = line_of(src, "$s = get_magic");
        let mut fs = vec![pf(rule, "error", "g.php", line)];
        full(fx.path(), &mut fs);
        assert_eq!(fs[0].severity, "error", "{name}");
    }
}

#[test]
fn f4_short_circuit_negated_guard_on_one_line() {
    let src = "<?php\nfunction f4( $v ) {\n\treturn ( ! function_exists( 'get_magic_quotes_gpc' ) || ! get_magic_quotes_gpc() ) ? addslashes( $v ) : $v;\n}\n";
    let fx = Fixture::new("f4").file("f4.php", src);
    let mut fs = vec![pf("FunctionUse.RemovedFunctions.get_magic_quotes_gpcDeprecatedRemoved", "error", "f4.php", 3)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    // `&&` after a negated check is not a guard: the call runs exactly when the function is missing.
    let src2 = "<?php\nif ( ! function_exists( 'get_magic_quotes_gpc' ) && get_magic_quotes_gpc() ) { $a = 1; }\n";
    let fx2 = Fixture::new("f4b").file("f4.php", src2);
    let mut fs2 = vec![pf("FunctionUse.RemovedFunctions.get_magic_quotes_gpcDeprecatedRemoved", "error", "f4.php", 2)];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "error");
}

#[test]
fn f5_else_branch_of_new_php_check_is_guarded() {
    let src = "<?php\nfunction f5( array $q ) {\n\tif ( version_compare( PHP_VERSION, '7.4', '>=' ) ) {\n\t\t$u = implode( '&', $q );\n\t} else {\n\t\t$u = implode( $q, '&' );\n\t}\n\treturn $u;\n}\n";
    let fx = Fixture::new("f5").file("f5.php", src);
    let mut fs = vec![pf("ParameterValues.RemovedImplodeFlexibleParamOrder.Removed", "error", "f5.php", 6)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    // After the whole if/else the code runs on every version again.
    let after = src.replace("\treturn $u;", "\t$z = implode( $q, '&' );\n\treturn $u;");
    let fx2 = Fixture::new("f5after").file("f5.php", &after);
    let mut fs2 = vec![pf("ParameterValues.RemovedImplodeFlexibleParamOrder.Removed", "error", "f5.php", line_of(&after, "$z = implode"))];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "error");
}

#[test]
fn f5_far_version_check_window_covers_long_blocks() {
    // flexible-shipping shape: the check is about 30 lines above, in both branches.
    let body: String = (0..28).map(|i| format!("\t\t$x{i} = 1;\n")).collect();
    let src = format!(
        "<?php\nfunction f5c( array $q ) {{\n\tif ( PHP_VERSION_ID > 50300 ) {{\n{body}\t\t$u = implode( '&', $q );\n\t}} else {{\n{body}\t\t$f = create_function( '', 'return 1;' );\n\t}}\n\treturn $u;\n}}\n"
    );
    let fx = Fixture::new("f5c").file("f5.php", &src);
    let mut fs = vec![pf("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "f5.php", line_of(&src, "create_function"))];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    // Old-PHP branch opened 40 lines above, finding still inside it.
    let long: String = (0..40).map(|i| format!("\t$x{i} = 1;\n")).collect();
    let src2 = format!("<?php\nif ( PHP_VERSION_ID < 70000 ) {{\n{long}\t$f = create_function( '', 'return 1;' );\n}}\n");
    let fx2 = Fixture::new("f5d").file("f5.php", &src2);
    let mut fs2 = vec![pf("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "f5.php", line_of(&src2, "create_function"))];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "info");
}

#[test]
fn f5b_removed_feature_in_new_php_branch_stays_error() {
    let src = "<?php\nfunction f5b() {\n\tif ( PHP_VERSION_ID >= 80000 ) {\n\t\t$f = create_function( '', 'return 1;' );\n\t\treturn $f();\n\t}\n\treturn 0;\n}\n";
    let fx = Fixture::new("f5b").file("f5b.php", src);
    let mut fs = vec![pf("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "f5b.php", 4)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
    assert_eq!(verdict_of(&fs), "breaks");
    // Old-side checks whose range still includes a checked target are not guards either.
    for cond in ["PHP_VERSION_ID < 90000", "version_compare( PHP_VERSION, '8.5', '<' )", "PHP_MAJOR_VERSION >= 8", "PHP_MAJOR_VERSION == 8"] {
        let s = src.replace("PHP_VERSION_ID >= 80000", cond);
        let fx = Fixture::new("f5bx").file("f5b.php", &s);
        let mut fs = vec![pf("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "f5b.php", 4)];
        full(fx.path(), &mut fs);
        assert_eq!(fs[0].severity, "error", "{cond}");
    }
    // Old-PHP checks in the other spellings are guards.
    for cond in ["PHP_MAJOR_VERSION < 8", "LIBXML_VERSION < 20900", "version_compare( PHP_VERSION, '7.0' ) < 0", "! ( PHP_VERSION_ID >= 70000 )"] {
        let s = src.replace("PHP_VERSION_ID >= 80000", cond);
        let fx = Fixture::new("f5by").file("f5b.php", &s);
        let mut fs = vec![pf("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "f5b.php", 4)];
        full(fx.path(), &mut fs);
        assert_eq!(fs[0].severity, "info", "{cond}");
    }
}

#[test]
fn f6_prefixed_defined_guard() {
    let src = "<?php\nnamespace Acme\\Vendor;\nfunction f6( $func, $m, $s, $k ) {\n\tswitch ( true ) {\n\t\tcase \\defined( 'Acme\\\\Vendor\\\\OPENSSL_PKCS1_PSS_PADDING' ):\n\t\t\treturn $func( $m, $s, $k, 'sha256', OPENSSL_PKCS1_PSS_PADDING );\n\t}\n\treturn false;\n}\n";
    let fx = Fixture::new("f6").file("f6.php", src);
    let mut fs = vec![pf("Constants.NewConstants.openssl_pkcs1_pss_paddingFound", "error", "f6.php", 6)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    assert_eq!(verdict_of(&fs), "compatible");
}

#[test]
fn f7_break_in_braceless_loop_body_is_dropped() {
    let src = "<?php\nfunction f7( array $args ) {\n\t$index = null;\n\tfor ( $i = 0; $i < count( $args ) - 1; $i++ ) if ( $args[ $i ] === 'plugin' ) { $index = $i; break; }\n\treturn $index;\n}\n";
    let fx = Fixture::new("f7").file("f7.php", src);
    let mut fs = vec![pf("ControlStructures.ForbiddenBreakContinueOutsideLoop.FatalError", "error", "f7.php", 4)];
    full(fx.path(), &mut fs);
    assert!(fs.is_empty());
    // A real break outside any loop stays.
    let fx2 = Fixture::new("f7b").file("f7.php", "<?php\nif ( $a ) {\n\tbreak;\n}\n");
    let mut fs2 = vec![pf("ControlStructures.ForbiddenBreakContinueOutsideLoop.FatalError", "error", "f7.php", 3)];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2.len(), 1);
}

#[test]
fn f8_namespaced_each_function_is_not_php_each() {
    let src = "<?php\nnamespace Acme\\Promise;\nfunction each( $iterable, callable $cb ) {\n\tforeach ( $iterable as $k => $v ) {\n\t\t$cb( $v, $k );\n\t}\n}\nfunction all( $promises ) {\n\t$r = array();\n\treturn each( $promises, function ( $v, $k ) use ( &$r ) { $r[ $k ] = $v; } );\n}\n";
    let fx = Fixture::new("f8").file("f8.php", src);
    let mut fs = vec![finding(EACH, "error", "f8.php", 10)];
    full(fx.path(), &mut fs);
    assert!(fs.is_empty());
    // Declared in another file of the same namespace (Guzzle's functions.php) also counts.
    let fx2 = Fixture::new("f8b")
        .file("functions.php", "<?php\nnamespace Acme\\Promise;\nfunction each( $i ) { return $i; }\n")
        .file("Each.php", "<?php\nnamespace Acme\\Promise;\nclass Each {\n\tfunction of( $p ) {\n\t\treturn each( $p );\n\t}\n}\n");
    let mut fs2 = vec![finding(EACH, "error", "Each.php", 5)];
    full(fx2.path(), &mut fs2);
    assert!(fs2.is_empty());
    // A namespaced file without its own each() still calls PHP's each().
    let fx3 = Fixture::new("f8c").file("x.php", "<?php\nnamespace Other;\nwhile ( list( $k, $v ) = each( $a ) ) {}\n");
    let mut fs3 = vec![finding(EACH, "error", "x.php", 3)];
    full(fx3.path(), &mut fs3);
    assert_eq!(fs3[0].severity, "error");
}

#[test]
fn f9_rules_that_do_not_break_on_upgrade() {
    let src = "<?php\nclass F9 {\n\tprivate static $c = array();\n\tpublic static function run( $k ) {\n\t\tini_set( 'magic_quotes_sybase', 0 );\n\t\t$h = strip_tags( '<b>x</b>', '<br/><b>' );\n\t\tif ( ! isset( $HTTP_RAW_POST_DATA ) ) {\n\t\t\t$HTTP_RAW_POST_DATA = file_get_contents( 'php://input' );\n\t\t}\n\t\treturn self::$c[ $k ]();\n\t}\n}\n";
    let fx = Fixture::new("f9").file("f9.php", src);
    let mut fs = vec![
        pf("IniDirectives.RemovedIniDirectives.magic_quotes_sybaseDeprecatedRemoved", "error", "f9.php", 5),
        pf("ParameterValues.ForbiddenStripTagsSelfClosingXHTML.Found", "error", "f9.php", 6),
        pf("Variables.RemovedPredefinedGlobalVariables.http_raw_post_dataDeprecatedRemoved", "error", "f9.php", 7),
        pf("Variables.RemovedPredefinedGlobalVariables.http_raw_post_dataDeprecatedRemoved", "error", "f9.php", 8),
        pf("Variables.NewUniformVariableSyntax.Found", "error", "f9.php", 10),
        pf("Variables.ForbiddenThisUseContexts.OutsideObjectContext", "error", "f9.php", 10),
        pf("ParameterValues.ChangedIntToBoolParamType.ob_implicit_flush_enableNumericFound", "error", "f9.php", 5),
    ];
    full(fx.path(), &mut fs);
    let got = sev(&fs);
    assert!(fs.iter().all(|f| f.severity != "error"), "{got:?}");
    let s = |r: &str| fs.iter().find(|f| f.rule_id.contains(r)).unwrap().severity.as_str();
    assert_eq!(s("RemovedIniDirectives"), "warning");
    // The file assigns $HTTP_RAW_POST_DATA itself (N14), so it does not rely on PHP filling it.
    assert_eq!(s("RemovedPredefinedGlobalVariables"), "info");
    assert_eq!(s("ForbiddenStripTags"), "info");
    assert_eq!(s("NewUniformVariableSyntax"), "info");
    assert_eq!(s("ChangedIntToBool"), "info");
    let this = fs.iter().find(|f| f.rule_id.contains("OutsideObjectContext")).unwrap();
    assert_eq!(this.severity, "info");
    assert!(this.message.contains("existing bug, not caused by the upgrade"), "{}", this.message);
    assert_eq!(verdict_of(&fs), "issues");
}

#[test]
fn f10_internal_notes_and_warning_noise() {
    let report = serde_json::json!({"totals": {}, "files": {
        "/r/index.php": {"messages": [{"source": "Internal.NoCodeFound", "type": "WARNING", "line": 1, "message": "No PHP code"}]},
        "/r/view.php": {"messages": [
            {"source": "Internal.LineEndings.Mixed", "type": "WARNING", "line": 1, "message": "mixed"},
            {"source": "PHPCompatibility.FunctionUse.RemovedFunctions.eachDeprecatedRemoved", "type": "ERROR", "line": 3, "message": "each"}
        ]}
    }});
    let fs = crate::php::parse_report(&report, Path::new("/r"), "php:phpcompatibility");
    let got: Vec<(&str, &str, &str)> = fs.iter().map(|f| (f.rule_id.as_str(), f.file.as_str(), f.severity.as_str())).collect();
    assert_eq!(got, vec![(EACH, "view.php", "error")]);
    // Non-deprecations that only produce warnings become info.
    let fx = Fixture::new("f10").file("a.php", "<?php\nclass A implements Serializable { function __x() {} function __serialize(): array { return []; } }\n");
    let mut fs = vec![
        pf("FunctionNameRestrictions.ReservedFunctionNames.MethodDoubleUnderscore", "warning", "a.php", 2),
        pf("Interfaces.RemovedSerializable.RedundantSerializableImplementation", "warning", "a.php", 2),
        pf("FunctionDeclarations.NewClosure.ThisFoundOutsideClass", "warning", "a.php", 2),
        pf("FunctionUse.ArgumentFunctionsReportCurrentValue.NeedsInspection", "warning", "a.php", 2),
        pf("Variables.ForbiddenGlobalVariableVariable.NonBareVariableFound", "warning", "a.php", 2),
    ];
    full(fx.path(), &mut fs);
    assert!(fs.iter().all(|f| f.severity == "info"), "{:?}", sev(&fs));
    assert_eq!(verdict_of(&fs), "compatible");
}

#[test]
fn f11_code_that_never_runs_is_not_scanned() {
    use crate::php::non_runtime_reason as nr;
    let php = b"<?php";
    for p in [
        "lib/x/test/HexTest.php",
        "lib/maennchen/zipstream-php/test/ZipTest.php",
        "php-toolkit/rowbot/url/tests/HostParserTest.php",
        "vendor/acme/lib/Tests/A.php",
        "vendor/tubalmartin/cssmin/gui/index.php",
        "vendor/acme/lib/examples/demo.php",
        "vendor/phplrt/compiler/resources/templates/parser.tpl.php",
        "assets/x.min.php",
        "node_modules/a/b.php",
    ] {
        assert!(nr(p, php).is_some(), "{p}");
    }
    assert_eq!(nr("vendor/swiftmailer/lib/swiftmailer_generate_mimes_config.php", b"#!/usr/bin/php\n<?php"), Some("command-line script"));
    for p in ["includes/class-test-mode.php", "includes/tests.php", "admin/gui/index.php", "templates/view.tpl.php", "vendor/acme/lib/src/Testing.php", "inc/latest/x.php"] {
        assert_eq!(nr(p, php), None, "{p}");
    }
    let fx = Fixture::new("f11")
        .file("lib/x/test/HexTest.php", "<?php\nclass HexTest {}\n")
        .file("bin/tool.php", "#!/usr/bin/env php\n<?php\n")
        .file("main.php", "<?php\n")
        .file("inc/a.inc", "<?php\n")
        .file("readme.txt", "x");
    let files: Vec<String> = crate::php::runtime_files(fx.path()).iter().map(|p| p.strip_prefix(fx.path()).unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(files, vec!["inc/a.inc", "main.php"]);
}

#[test]
fn guards_now_apply_to_warnings() {
    let src = "<?php\nif ( PHP_VERSION_ID < 80000 ) {\n\tlibxml_disable_entity_loader( true );\n}\n";
    let fx = Fixture::new("warnguard").file("x.php", src);
    let mut fs = vec![pf("FunctionUse.RemovedFunctions.libxml_disable_entity_loaderDeprecated", "warning", "x.php", 3)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
}

#[test]
fn duplicate_findings_are_reported_once() {
    let fx = Fixture::new("dup").file("a.php", "<?php\n$x = 1;\n");
    let mut fs = vec![finding(CREATE_FUNCTION, "error", "a.php", 2), finding(CREATE_FUNCTION, "error", "a.php", 2), finding(CREATE_FUNCTION, "error", "a.php", 3)];
    full(fx.path(), &mut fs);
    assert_eq!(fs.len(), 2);
}

#[test]
fn components_are_deduplicated() {
    let c = |name: &str, v: Option<&str>, path: &str, det: &str, kind: &str| {
        let mut x = comp(name, v, path);
        x.detection = det.into();
        x.kind = kind.into();
        x
    };
    let comps = vec![
        c("guzzle", Some("7"), "vendor/guzzlehttp/guzzle/src", "signature", "php"),
        c("guzzlehttp/guzzle", Some("7.10.3"), "vendor/guzzlehttp/guzzle", "composer-installed", "php"),
        c("monolog", Some("2"), "vendor/monolog/monolog/src/Monolog", "signature", "php"),
        c("monolog/monolog", Some("2.11.1"), "vendor/monolog/monolog", "composer-installed", "php"),
        c("phpseclib", None, "vendor/phpseclib/phpseclib/phpseclib", "signature", "php"),
        c("phpseclib/phpseclib", Some("3.0.57"), "vendor/phpseclib/phpseclib", "composer-installed", "php"),
        // A different copy elsewhere in the plugin with another version is kept.
        c("phpmailer", Some("5.2.7"), "lib/PHPMailer", "signature", "php"),
        c("phpmailer", Some("6.9.1"), "vendor/phpmailer/phpmailer/src", "signature", "php"),
        // The same JS library once per file: keep the shortest path.
        c("bootstrap", Some("4.6.0"), "assets/js/vendor/bootstrap.bundle.min.js", "retirejs", "js"),
        c("bootstrap", Some("4.6.0"), "assets/js/bootstrap.min.js", "retirejs", "js"),
        c("bootstrap", Some("4.6.0"), "admin/assets/js/bootstrap.bundle.js", "retirejs", "js"),
    ];
    let got: Vec<String> = detect::dedupe_components(comps).iter().map(|c| format!("{} {} {}", c.name, c.version.as_deref().unwrap_or("-"), c.path)).collect();
    assert_eq!(
        got,
        vec![
            "bootstrap 4.6.0 assets/js/bootstrap.min.js",
            "guzzlehttp/guzzle 7.10.3 vendor/guzzlehttp/guzzle",
            "monolog/monolog 2.11.1 vendor/monolog/monolog",
            "phpmailer 5.2.7 lib/PHPMailer",
            "phpmailer 6.9.1 vendor/phpmailer/phpmailer/src",
            "phpseclib/phpseclib 3.0.57 vendor/phpseclib/phpseclib",
        ]
    );
}

#[test]
fn suppression_info_action_and_prefix_rules() {
    let fx = Fixture::new("suppinfo").file(
        "suppressions.json",
        r#"[{"rule_id":"R.ini.*","path_regex":".*","action":"info","reason":"no-op"},{"rule_id":"R.exact","path_regex":".*","action":"info","reason":"x"}]"#,
    );
    let mut fs = vec![finding("R.ini.a", "error", "a.php", 1), finding("R.ini.b", "warning", "a.php", 2), finding("R.exact.more", "error", "a.php", 3), finding("R.ini.c", "info", "a.php", 4)];
    detect::apply_suppressions(fx.path(), &mut fs);
    let got: Vec<&str> = fs.iter().map(|f| f.severity.as_str()).collect();
    assert_eq!(got, vec!["info", "info", "error", "info"]);
    assert_eq!(fs[0].message, "test message (no-op)");
    assert_eq!(fs[3].message, "test message", "already-info findings are unchanged");
}

#[test]
fn file_selected_by_old_php_version_check_becomes_info() {
    let loader = "<?php\nrequire_once plugin_dir_path( __FILE__ ) . ( version_compare( PHP_VERSION, '5.3', '<' ) ? 'includes/sc-php52.php' : 'includes/sc.php' );\n";
    let fx = Fixture::new("d5").file("main.php", loader).file("includes/sc-php52.php", "<?php\n$f = create_function( '', 'return 1;' );\n").file("includes/sc.php", "<?php\n$f = static function () {};\n");
    let mut fs = vec![finding(CREATE_FUNCTION, "error", "includes/sc-php52.php", 2), finding(CREATE_FUNCTION, "error", "includes/sc.php", 2)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    assert_eq!(fs[1].severity, "error", "the new-PHP side is loaded on new PHP");
    // Selected on the new-PHP side, or also included unconditionally elsewhere: stays an error.
    for extra in ["<?php\nrequire 'includes/sc-php52.php';\n", "<?php\nrequire PHP_VERSION_ID >= 80000 ? 'includes/sc-php52.php' : 'x.php';\n"] {
        let fx = Fixture::new("d5b").file("main.php", loader).file("other.php", extra).file("includes/sc-php52.php", "<?php\n$f = create_function( '', 'return 1;' );\n");
        let mut fs = vec![finding(CREATE_FUNCTION, "error", "includes/sc-php52.php", 2)];
        full(fx.path(), &mut fs);
        assert_eq!(fs[0].severity, "error", "{extra}");
    }
    // A file nobody includes by name keeps its findings.
    let fx = Fixture::new("d5c").file("a.php", "<?php\n$f = create_function( '', 'return 1;' );\n");
    let mut fs = vec![finding(CREATE_FUNCTION, "error", "a.php", 2)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
}

#[test]
fn feature_checks_do_not_guard_deprecation_warnings() {
    // The deprecated constant still exists on PHP 8.4, so this code runs and is still deprecated.
    let src = "<?php\nif ( defined( 'MYSQLI_STORE_RESULT_COPY_DATA' ) ) {\n\treturn mysqli_store_result( $l, MYSQLI_STORE_RESULT_COPY_DATA );\n}\n";
    let fx = Fixture::new("depwarn").file("a.php", src);
    let mut fs = vec![pf("Constants.RemovedConstants.mysqli_store_result_copy_dataDeprecated", "warning", "a.php", 3)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "warning");
}

// ---------- ACC-1b fixtures (each one our own minimal code) ----------

const BREAK_OUTSIDE: &str = "ControlStructures.ForbiddenBreakContinueOutsideLoop.FatalError";

#[test]
fn n1_polyfill_stub_files_are_info() {
    let engine = "<?php\nnamespace Random;\nif (\\PHP_VERSION_ID < 80200) {\n    interface Engine { public function generate(): string; }\n}\n";
    let duration = "<?php\nnamespace Time;\nif (\\PHP_VERSION_ID < 80600) {\n    final class Duration {\n        public static function ofSeconds(int $s): Duration { return new Duration(); }\n    }\n}\n";
    let e = "vendor/symfony/polyfill-php82/Resources/stubs/Random/Engine.php";
    let d = "vendor/symfony/polyfill-time/Resources/stubs/Time/Duration.php";
    let b = "vendor/symfony/polyfill-php85/bootstrap.php";
    let m = "modules/Symfony/Polyfill/Php82/Resources/stubs/Random/Engine.php";
    let fx = Fixture::new("n1").file(e, engine).file(d, duration).file(b, "<?php\nfunction array_first(array $a) { return 1; }\n").file(m, engine);
    let mut fs = vec![
        pf("Namespaces.ReservedNames.randomFound", "error", e, 2),
        pf("Classes.NewClasses.time_durationFound", "error", d, 5),
        pf("Classes.NewClasses.x_Found", "error", b, 2),
        pf("Namespaces.ReservedNames.randomFound", "error", m, 2),
    ];
    full(fx.path(), &mut fs);
    assert!(fs.iter().all(|f| f.severity == "info"), "{:?}", sev(&fs));
    assert_eq!(verdict_of(&fs), "compatible");
}

#[test]
fn n1_new_class_behind_new_php_check_is_guarded() {
    // Guzzle-style: the class is only used when PHP 8.6+ has it.
    let src = "<?php\nfinal class H {\n    private static function hasStreamError(): bool\n    {\n        return \\PHP_VERSION_ID >= 80600 && \\class_exists(\\StreamError::class, false);\n    }\n    function a() {\n        if (\\PHP_VERSION_ID >= 80600) {\n            $e = new \\Uri\\Rfc3986\\Uri('x');\n        }\n    }\n    function b() {\n        $e = new \\Uri\\Rfc3986\\Uri('x');\n    }\n}\n";
    let fx = Fixture::new("n1b").file("StreamHandler.php", src);
    let mut fs = vec![
        pf("Classes.NewClasses.streamerrorFound", "error", "StreamHandler.php", 5),
        pf("Classes.NewClasses.uri_rfc3986_uriFound", "error", "StreamHandler.php", 9),
        pf("Classes.NewClasses.uri_rfc3986_uriFound", "error", "StreamHandler.php", 13),
    ];
    full(fx.path(), &mut fs);
    assert_eq!(sev(&fs).iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), vec!["info", "info", "error"], "{:?}", sev(&fs));
    // A check that also allows the target version (>= 8.0) does not guard a newer feature.
    let fx2 = Fixture::new("n1c").file("a.php", "<?php\nif (PHP_VERSION_ID >= 80000) {\n    $d = new \\Uri\\Rfc3986\\Uri('x');\n}\n");
    let mut fs2 = vec![pf("Classes.NewClasses.uri_rfc3986_uriFound", "error", "a.php", 3)];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "error");
}

#[test]
fn n2_hex_string_literals_are_info() {
    let src = "<?php\n$color = \"0x142864\";\n$rgb = hexdec($color);\n$patterns = array('\\(0x', '0x3c62723e', '0x00');\n";
    let fx = Fixture::new("n2").file("plugin.php", src);
    let r = "Numbers.RemovedHexadecimalNumericStrings.Found";
    let mut fs = vec![pf(r, "error", "plugin.php", 2), pf(r, "error", "plugin.php", 4), pf(r, "error", "plugin.php", 4)];
    full(fx.path(), &mut fs);
    assert!(fs.iter().all(|f| f.severity == "info"), "{:?}", sev(&fs));
    assert!(fs[0].message.contains("PHP 7.0 change"), "{}", fs[0].message);
    assert_eq!(verdict_of(&fs), "compatible");
}

#[test]
fn n3_break_in_braceless_loop_bodies_is_dropped() {
    let src = "<?php\nfunction first_admin($roles) {\n    $access = false;\n    foreach ($roles as $name)\n        if ($name === 'administrator') {\n            $access = true;\n            break;\n        }\n    return $access;\n}\nfunction next_same($nd) {\n    $t = $nd->nodeType;\n    while ($nd = $nd->nextSibling)\n        if ($nd->nodeType === $t)\n            break;\n    return $nd;\n}\nfunction serve($s) {\n    while (true) try {\n        if (!$s->ok()) { continue; };\n        return 1;\n    } catch (Exception $e) { return 0; }\n}\nfunction chain($a) {\n    for ($i = 0; $i < 3; $i++)\n        if ($a) { $x = 1; }\n        elseif ($i) { $x = 2; }\n        else { break; }\n}\n";
    let fx = Fixture::new("n3").file("plugin.php", src);
    let mut fs = vec![pf(BREAK_OUTSIDE, "error", "plugin.php", 7), pf(BREAK_OUTSIDE, "error", "plugin.php", 15), pf(BREAK_OUTSIDE, "error", "plugin.php", 20), pf(BREAK_OUTSIDE, "error", "plugin.php", 28)];
    full(fx.path(), &mut fs);
    assert!(fs.is_empty(), "{:?}", fs.iter().map(|f| f.line).collect::<Vec<_>>());
}

#[test]
fn n3_break_after_a_braceless_loop_body_ended_stays() {
    // The loop body is only `foo();`; the break after it is outside any loop (a real fatal error).
    let src = "<?php\nfunction f($a) {\n    while ($a)\n        foo();\n    break;\n}\nif ($a) {\n    $b = 1;\n    continue;\n}\nfunction g($a) { if ($a) { return; } else { break; } }\n";
    let fx = Fixture::new("n3b").file("plugin.php", src);
    let mut fs = vec![pf(BREAK_OUTSIDE, "error", "plugin.php", 5), pf(BREAK_OUTSIDE, "error", "plugin.php", 9), pf(BREAK_OUTSIDE, "error", "plugin.php", 11)];
    full(fx.path(), &mut fs);
    assert_eq!(fs.len(), 3);
    assert_eq!(verdict_of(&fs), "breaks");
}

#[test]
fn n4_ternary_rule_inside_match_arm_is_info() {
    let src = "<?php\nclass R {\n    function resolve($id, $s, $c) {\n        return match ($id) {\n            'a' => $s->with([\n                'reason' => ! $c->ok()\n                    ? 'No permission.'\n                    : ($c->live()\n                        ? 'Nothing to publish.'\n                        : null),\n            ]),\n            default => $s,\n        };\n    }\n    function other($a, $b) {\n        return $a ? 1 : $b ? 2 : 3;\n    }\n    function m($q) { return $this->match($q) ? 1 : 2; }\n}\n";
    let fx = Fixture::new("n4").file("plugin.php", src);
    let r = "Operators.RemovedTernaryAssociativity.Removed";
    let mut fs = vec![pf(r, "error", "plugin.php", 7), pf(r, "error", "plugin.php", 16)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    assert!(fs[0].message.contains("match expression"), "{}", fs[0].message);
    assert_eq!(fs[1].severity, "error", "outside a match block the rule still applies");
}

#[test]
fn n5_global_dynamic_variable_is_not_this() {
    let src = "<?php\nclass GlobalVar {\n    private $name;\n    public function __construct($n) { $this->name = $n; }\n    public function __invoke() { global ${$this->name}; return ${$this->name}; }\n}\nfunction bad() { global $this; }\n";
    let fx = Fixture::new("n5").file("plugin.php", src);
    let mut fs = vec![
        pf("Variables.ForbiddenThisUseContexts.Global", "error", "plugin.php", 5),
        pf("Variables.ForbiddenGlobalVariableVariable.NonBareVariableFound", "warning", "plugin.php", 5),
        pf("Variables.ForbiddenThisUseContexts.Global", "error", "plugin.php", 7),
    ];
    full(fx.path(), &mut fs);
    assert_eq!(sev(&fs), vec![("Variables.ForbiddenGlobalVariableVariable.NonBareVariableFound".into(), "info".into()), ("Variables.ForbiddenThisUseContexts.Global".into(), "error".into())]);
    assert_eq!(fs[1].line, 7);
}

#[test]
fn n6_datetimeinterface_interface_dropped_class_info() {
    let src = "<?php\ninterface MyDateInterface extends DateTimeInterface, JsonSerializable {}\nif (!class_exists('DateTimeImmutable')) {\n    class DateTimeImmutable implements DateTimeInterface {}\n}\n";
    let fx = Fixture::new("n6").file("plugin.php", src);
    let mut fs = vec![finding(CARBON_RULE, "error", "plugin.php", 2), finding(CARBON_RULE, "error", "plugin.php", 4)];
    full(fx.path(), &mut fs);
    assert_eq!(fs.len(), 1);
    assert_eq!((fs[0].line, fs[0].severity.as_str()), (4, "info"));
    assert_eq!(verdict_of(&fs), "compatible");
}

#[test]
fn n7_define_third_argument_false_or_variable() {
    let src = "<?php\ndefine('ACME_PREFIX', 'acme.', false);\nfunction d($n, $v, $ci = false) { return define($n, $v, $ci); }\ndefine('ACME_X', 'x', true);\n";
    let fx = Fixture::new("n7").file("plugin.php", src);
    let r = "FunctionUse.RemovedFunctionParameters.define_case_insensitiveDeprecatedRemoved";
    let mut fs = vec![pf(r, "error", "plugin.php", 2), pf(r, "error", "plugin.php", 3), pf(r, "error", "plugin.php", 4)];
    full(fx.path(), &mut fs);
    let got: Vec<(u32, &str)> = fs.iter().map(|f| (f.line, f.severity.as_str())).collect();
    assert_eq!(got, vec![(3, "info"), (4, "error")]);
}

#[test]
fn n8_safe_generated_wrappers_are_info() {
    let src = "<?php\nnamespace Safe;\nfunction oci_bind_array_by_name($statement, string $name, array &$var, int $max, int $len = -1, int $type = SQLT_AFC): void {}\n";
    let fx = Fixture::new("n8").file("lib/Safe/generated/oci8.php", src);
    let mut fs = vec![pf("Constants.RemovedConstants.sqlt_afcRemoved", "error", "lib/Safe/generated/oci8.php", 3)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    assert_eq!(verdict_of(&fs), "compatible");
}

#[test]
fn n9_composer_build_time_files_are_not_scanned() {
    use crate::php::non_runtime_reason as nr;
    for p in [
        "vendor/composer/platform_check.php",
        "vendor/composer/installers/src/Composer/Installers/BaseInstaller.php",
        "vendor_prefixed/composer/installers/src/Composer/Installers/Installer.php",
        "lib/vendor/composer/platform_check.php",
    ] {
        assert!(nr(p, b"<?php").is_some(), "{p}");
    }
    for p in ["vendor/composer/autoload_real.php", "vendor/composer/ClassLoader.php", "vendor/acme/composer/platform_check.php", "inc/composer/installers/x.php"] {
        assert_eq!(nr(p, b"<?php"), None, "{p}");
    }
    let fx = Fixture::new("n9")
        .file("vendor/composer/installers/src/Composer/Installers/BaseInstaller.php", "<?php\nabstract class BaseInstaller {}\n")
        .file("vendor/composer/platform_check.php", "<?php\n$issues = array();\n")
        .file("vendor/composer/autoload_real.php", "<?php\n");
    let files: Vec<String> = crate::php::runtime_files(fx.path()).iter().map(|p| p.strip_prefix(fx.path()).unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(files, vec!["vendor/composer/autoload_real.php"]);
}

#[test]
fn n10_each_imported_with_use_function_is_not_php_each() {
    let fx = Fixture::new("n10")
        .file("src/each.php", "<?php\nnamespace _;\nfunction each($collection, callable $it) { return $collection; }\n")
        .file("src/internal/agg.php", "<?php\nnamespace _\\internal;\nuse function _\\each;\nfunction agg($c, $f) { each($c, function ($v) use ($f) { $f($v); }); }\n")
        .file("src/grp.php", "<?php\nnamespace B;\nuse function _\\{map, each};\nfunction g($c) { return each($c, 'f'); }\n")
        .file("src/alias.php", "<?php\nnamespace C;\nuse function _\\each as walk;\nfunction h($a) { return each($a); }\n");
    let mut fs = vec![finding(EACH, "error", "src/internal/agg.php", 4), finding(EACH, "error", "src/grp.php", 4), finding(EACH, "error", "src/alias.php", 4)];
    full(fx.path(), &mut fs);
    assert_eq!(fs.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(), vec!["src/alias.php"], "an aliased import leaves each() as PHP's own");
}

#[test]
fn n12_function_exists_dynamic_early_return() {
    let src = "<?php\nclass Md {\n    public $utf8_strlen = 'mb_strlen';\n    function setup() {\n        if (function_exists($this->utf8_strlen)) return;\n        $this->utf8_strlen = create_function('$text', 'return strlen($text);');\n    }\n}\n";
    let fx = Fixture::new("n12").file("plugin.php", src);
    let mut fs = vec![finding(CREATE_FUNCTION, "error", "plugin.php", 6)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    // Default names a function that may be missing: not a guard.
    let other = src.replace("'mb_strlen'", "'iconv_strlen'");
    let fx2 = Fixture::new("n12b").file("plugin.php", &other);
    let mut fs2 = vec![finding(CREATE_FUNCTION, "error", "plugin.php", 6)];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "error");
}

#[test]
fn n13_exit_in_constructor_without_destructor_is_info() {
    let src = "<?php\nclass Export extends Base_Request {\n    public function __construct() {\n        if (!current_user_can('manage_options')) { exit; }\n    }\n}\n";
    let r = "FunctionDeclarations.RemovedCallingDestructAfterConstructorExit.NeedsInspection";
    let fx = Fixture::new("n13").file("plugin.php", src);
    let mut fs = vec![pf(r, "warning", "plugin.php", 4)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    assert_eq!(verdict_of(&fs), "compatible");
    // A destructor anywhere in the package keeps the warning.
    let fx2 = Fixture::new("n13b").file("plugin.php", src).file("inc/base.php", "<?php\nclass Base_Request {\n    public function __destruct() { log_it(); }\n}\n");
    let mut fs2 = vec![pf(r, "warning", "plugin.php", 4)];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "warning");
}

#[test]
fn n14_http_raw_post_data_set_by_the_file_is_info() {
    let r = "Variables.RemovedPredefinedGlobalVariables.http_raw_post_dataDeprecatedRemoved";
    let fx = Fixture::new("n14")
        .file("a.php", "<?php\nif (!isset($HTTP_RAW_POST_DATA)) {\n    $HTTP_RAW_POST_DATA = file_get_contents('php://input');\n}\n$x = $HTTP_RAW_POST_DATA;\n")
        .file("b.php", "<?php\nif ($HTTP_RAW_POST_DATA == '') { return; }\n$x = $HTTP_RAW_POST_DATA;\n");
    let mut fs = vec![pf(r, "error", "a.php", 5), pf(r, "error", "b.php", 3)];
    full(fx.path(), &mut fs);
    assert_eq!(sev(&fs).iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), vec!["info", "warning"]);
}

#[test]
fn n15_package_with_its_own_array_first_needs_no_wordpress_polyfill() {
    let r = "FunctionUse.NewFunctions.array_firstFound";
    let uses = "<?php\n/* Plugin Name: X */\n$a = array_first($list);\n";
    // symfony/polyfill-php85 style bootstrap in the global namespace.
    let fx = Fixture::new("n15").file("x.php", uses).file("vendor/acme/compat/functions.php", "<?php\nif (!function_exists('array_first')) {\n    function array_first(array $array) { return $array ? reset($array) : null; }\n}\n");
    let mut fs = vec![pf(r, "error", "x.php", 3)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info", "{}", fs[0].message);
    // Imported from the plugin's own namespace.
    let fx2 = Fixture::new("n15b").file("x.php", "<?php\nnamespace Kirki\\Q;\nuse function Kirki\\Polyfill\\array_first;\n$a = array_first($l);\n");
    let mut fs2 = vec![pf(r, "error", "x.php", 4)];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "info");
    // A method named array_first in a class, or nothing at all: still needs WordPress 6.9+.
    let fx3 = Fixture::new("n15c").file("x.php", uses).file("inc/c.php", "<?php\nclass C { function array_first($a) { return 1; } }\n");
    let mut fs3 = vec![pf(r, "error", "x.php", 3)];
    full(fx3.path(), &mut fs3);
    assert_eq!(fs3[0].severity, "warning");
    assert!(fs3[0].message.starts_with("Needs WordPress 6.9+"), "{}", fs3[0].message);
}

#[test]
fn c1_real_breaks_still_break() {
    let src = "<?php\nfunction bad($q) { return implode($q, '&'); }\n$s = \"abc\"; echo $s{0};\n";
    let fx = Fixture::new("c1").file("plugin.php", src);
    let mut fs = vec![pf("ParameterValues.RemovedImplodeFlexibleParamOrder.Removed", "error", "plugin.php", 2), pf("Syntax.RemovedCurlyBraceArrayAccess.Removed", "error", "plugin.php", 3)];
    full(fx.path(), &mut fs);
    assert!(fs.iter().all(|f| f.severity == "error"), "{:?}", sev(&fs));
    assert_eq!(verdict_of(&fs), "breaks");
}

// ---------- class H: errors only in bundled libraries ----------

fn stored(per_target: &[(String, Vec<Finding>)], encoded: u32) -> Vec<(String, String, i64, Vec<String>)> {
    let fx = Fixture::new("dbh");
    let db = fx.path().join("t.sqlite");
    let conn = crate::db::open(db.to_str().unwrap()).unwrap();
    let run = crate::trace::Run::start(&conn, "test", serde_json::json!({})).unwrap();
    crate::store(&conn, &run.id, "wordpress-plugin", "fixture", "1.0", &[], &[], per_target, encoded, &|_| false).unwrap();
    let mut stmt = conn.prepare("SELECT id, target, verdict, errors FROM result ORDER BY target").unwrap();
    let rows: Vec<(i64, String, String, i64)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().map(|r| r.unwrap()).collect();
    rows.into_iter()
        .map(|(id, t, v, e)| {
            let mut s = conn.prepare("SELECT message FROM finding WHERE result_id=?1 ORDER BY id").unwrap();
            let msgs = s.query_map([id], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
            (t, v, e, msgs)
        })
        .collect()
}

fn in_lib(mut f: Finding, comp: &str) -> Finding {
    f.component = Some(comp.into());
    f
}

#[test]
fn h_errors_only_in_bundled_components_give_issues() {
    let lib = |l| in_lib(finding(EACH, "error", "lib/ftp.php", l), "ftp-lib@1.0");
    let per_target = vec![
        ("8.3".to_string(), vec![lib(1), lib(2), finding(EACH, "warning", "main.php", 3)]),
        ("8.4".to_string(), vec![lib(1), finding(CREATE_FUNCTION, "error", "main.php", 9)]),
        ("8.5".to_string(), vec![lib(1)]),
    ];
    let got = stored(&per_target, 0);
    let p = crate::BUNDLED_PREFIX;
    assert_eq!(got[0].1, "issues");
    assert_eq!(got[0].2, 2, "errors are still counted");
    assert_eq!(got[0].3, vec![format!("{p}test message"), format!("{p}test message"), "test message".to_string()]);
    assert_eq!(got[1].1, "breaks", "an error in the plugin's own code keeps breaks");
    assert_eq!(got[1].3, vec!["test message".to_string(), "test message".to_string()], "no prefix when the verdict is breaks");
    assert_eq!(got[2].1, "issues");
    // Never compatible; encoded files still make it unknown.
    let enc = stored(&[("8.4".to_string(), vec![lib(1)])], 1);
    assert_eq!(enc[0].1, "unknown");
}

// ---------- minor fixes ----------

#[test]
fn encoded_file_detection_needs_a_loader_stub() {
    use crate::is_encoded_php as enc;
    assert!(enc("<?php //0046a\nif(!extension_loaded('ionCube Loader')){$__oc=strtolower(substr(php_uname(),0,3));$__ln='ioncube_loader_'.$__oc.'_'.substr(phpversion(),0,3).(($__oc=='win')?'.dll':'.so');}"));
    assert!(enc("<?php //ICB0 81:0 82:b4a\nif(extension_loaded('ionCube Loader')){die('The file '.__FILE__.\" is corrupted.\\n\");}"));
    assert!(enc("<?php @\"SourceGuardian\"; //v12.1.2\nif(!function_exists('sg_load')){$__v=phpversion();}return sg_load('52C4625FB82E51A5AAQAAAAXAAAABHAAAACABAAAAAAAAAD/');"));
    // easy-wp-smtp 2.x ships ionCube's open-source openssl Cryptor: not encoded.
    assert!(!enc("<?php\n/**\n * Copyright (c) 2016 ionCube Ltd.\n */\nnamespace ioncube\\phpOpensslCryptor;\nclass Cryptor {}\n"));
    assert!(!enc("<?php\nuse ioncube\\phpOpensslCryptor\\Cryptor;\n// requires SourceGuardian? no\n"));
}

#[test]
fn google_apiclient_signature_is_merged_into_composer_entry() {
    let c = |name: &str, v: &str, path: &str, det: &str| {
        let mut x = comp(name, Some(v), path);
        x.detection = det.into();
        x
    };
    let comps = vec![
        c("google-api-client", "2.12.6", "vendor/google/apiclient/src", "signature"),
        c("google/apiclient", "2.12.6", "vendor/google/apiclient", "composer-installed"),
    ];
    let got: Vec<String> = detect::dedupe_components(comps).iter().map(|c| c.name.clone()).collect();
    assert_eq!(got, vec!["google/apiclient"]);
}

#[test]
fn h_hand_confirmed_bundled_break_keeps_breaks() {
    let rules = rules();
    let rule = "PHPCompatibility.ParameterValues.RemovedImplodeFlexibleParamOrder.Removed";
    let f = in_lib(finding(rule, "error", "lib/Google/Http/REST.php", 129), "google-api-client@1.1.0-beta");
    assert!(rules.is_confirmed_break("w3-total-cache", &f));
    assert!(!rules.is_confirmed_break("other-plugin", &f));
    let fx = Fixture::new("dbc");
    let conn = crate::db::open(fx.path().join("t.sqlite").to_str().unwrap()).unwrap();
    let run = crate::trace::Run::start(&conn, "test", serde_json::json!({})).unwrap();
    let per_target = vec![("8.4".to_string(), vec![f])];
    crate::store(&conn, &run.id, "wordpress-plugin", "w3-total-cache", "2.11.0", &[], &[], &per_target, 0, &|x| rules.is_confirmed_break("w3-total-cache", x)).unwrap();
    let (v, m): (String, String) = conn.query_row("SELECT r.verdict, f.message FROM result r JOIN finding f ON f.result_id = r.id", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((v.as_str(), m.as_str()), ("breaks", "test message"));
}

#[test]
fn requests_and_aws_sdk_v1_signatures_cover_their_files() {
    let fx = Fixture::new("sigs")
        .file("sdk/libs/Requests-1.6.1/library/Requests.php", "<?php\nclass Requests {\n\tconst VERSION = '1.6.1';\n}\n")
        .file("sdk/libs/Requests-1.6.1/library/Requests/Transport/fsockopen.php", "<?php\n")
        .file("lib/aws/sdk.class.php", "<?php\ndefine('CFRUNTIME_NAME', 'aws-sdk-php');\ndefine('CFRUNTIME_VERSION', '1.5.14');\n")
        .file("lib/aws/lib/requestcore/requestcore.class.php", "<?php\nclass RequestCore {}\n")
        .file("includes/Requests.php", "<?php\nclass My_Requests { const VERSION = '9'; }\n");
    let comps = detect::dedupe_components(detect::components(fx.path(), &rules()));
    let got: Vec<String> = comps.iter().map(|c| format!("{} {}", c.name, c.version.as_deref().unwrap_or("-"))).collect();
    assert_eq!(got, vec!["aws-sdk-php 1.5.14", "requests 1.6.1"]);
    assert_eq!(detect::attribute("sdk/libs/Requests-1.6.1/library/Requests/Transport/fsockopen.php", &comps).as_deref(), Some("requests@1.6.1"));
    assert_eq!(detect::attribute("lib/aws/lib/requestcore/requestcore.class.php", &comps).as_deref(), Some("aws-sdk-php@1.5.14"));
}

#[test]
fn strip_comments_ignores_comment_markers_inside_strings() {
    let src = "<?php\n$files = glob( $dir . '/*' );\n$c = \"#fff\"; $u = 'http://x'; // note\n/* real\n comment */ $x = 1; # hash\n$h = <<<EOT\nit's /* not a comment\nEOT;\n?>\n<p>Don't /* html */</p>\n<?php $y = 2; ?>\n";
    let out = detect::strip_comments(src);
    assert_eq!(out.lines().count(), src.lines().count());
    let want = "<?php\n$files = glob( $dir . '/*' );\n$c = \"#fff\"; $u = 'http://x'; \n\n $x = 1; \n$h = <<<EOT\nit's /* not a comment\nEOT;\n?>\n<p>Don't /* html */</p>\n<?php $y = 2; ?>\n";
    assert_eq!(out, want);
    // A `'/*'` string earlier in the file must not hide a braceless loop body (seraphinite-accelerator).
    let loop_src = "<?php\nfunction f($d, $id) {\n\t$a = glob( $d . '/*' );\n\tforeach( glob( $d . '/' . bin2hex( $id ) . '.*', GLOB_NOSORT ) as $file )\n\t\tbreak;\n\treturn $file;\n}\n";
    let fx = Fixture::new("strip").file("common.php", loop_src);
    let mut fs = vec![pf(BREAK_OUTSIDE, "error", "common.php", 5)];
    full(fx.path(), &mut fs);
    assert!(fs.is_empty());
}

// ---------- fixtures n11 and n16-n27, c2 (warning noise, misreads, guards, never-reached code) ----------

/// A finding with the analyzer's real message (some passes read the "since PHP X.Y" in it).
fn pfm(rule: &str, severity: &str, file: &str, line: u32, message: &str) -> Finding {
    let mut f = pf(rule, severity, file, line);
    f.message = message.into();
    f
}

const IMPLODE_RULE: &str = "ParameterValues.RemovedImplodeFlexibleParamOrder.Removed";
const IMPLODE_MSG: &str = "Passing the $glue and $pieces parameters in reverse order to implode has been deprecated since PHP 7.4 and is removed since PHP 8.0; $glue should be the first parameter and $pieces the second";
const CREATE_FUNCTION_MSG: &str = "Function create_function() is deprecated since PHP 7.2 and removed since PHP 8.0; Use an anonymous function instead";
const EACH_MSG: &str = "Function each() is deprecated since PHP 7.2 and removed since PHP 8.0; Use a foreach loop or ArrayIterator instead";
const E_STRICT_RULE: &str = "Constants.RemovedConstants.e_strictDeprecated";
const E_STRICT_MSG: &str = "The constant \"E_STRICT\" is deprecated since PHP 8.4";

#[test]
fn n11_version_check_ternary_guards_its_old_side() {
    let src = "<?php\n$levels = version_compare(PHP_VERSION, '8.4.0', '>=') ? E_ALL : E_ALL & ~E_STRICT;\nset_error_handler('my_handler', $levels);\n";
    let fx = Fixture::new("n11").file("plugin.php", src);
    let mut fs = vec![pfm(E_STRICT_RULE, "warning", "plugin.php", 2, E_STRICT_MSG)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info", "{}", fs[0].message);
    assert_eq!(verdict_of(&fs), "compatible");
    // E_STRICT on the new-PHP side, or a check whose old side still includes PHP 8.4: kept.
    for (name, src) in [
        ("n11b", "<?php\n$levels = version_compare(PHP_VERSION, '8.4.0', '<') ? E_ALL : E_ALL & ~E_STRICT;\n"),
        ("n11c", "<?php\n$levels = PHP_VERSION_ID >= 80500 ? E_ALL : E_ALL & ~E_STRICT;\n"),
        ("n11d", "<?php\n$levels = PHP_VERSION_ID >= 80400 && $x ? E_ALL : E_ALL & ~E_STRICT;\n"),
        ("n11e", "<?php\n$levels = E_ALL & ~E_STRICT | (PHP_VERSION_ID >= 80400 ? 1 : 2);\n"),
    ] {
        let fx = Fixture::new(name).file("plugin.php", src);
        let mut fs = vec![pfm(E_STRICT_RULE, "warning", "plugin.php", 2, E_STRICT_MSG)];
        full(fx.path(), &mut fs);
        assert_eq!(fs[0].severity, "warning", "{name}");
    }
    // Multi-line ternary, and a plain `PHP_VERSION_ID < 80400` block.
    let fx = Fixture::new("n11f")
        .file("a.php", "<?php\n$l = PHP_VERSION_ID >= 80400\n    ? E_ALL\n    : E_ALL & ~E_STRICT;\n")
        .file("b.php", "<?php\nif (PHP_VERSION_ID < 80400) {\n    error_reporting(E_ALL & ~E_STRICT);\n}\n");
    let mut fs = vec![pfm(E_STRICT_RULE, "warning", "a.php", 4, E_STRICT_MSG), pfm(E_STRICT_RULE, "warning", "b.php", 3, E_STRICT_MSG)];
    full(fx.path(), &mut fs);
    assert_eq!(sev(&fs).iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), vec!["info", "info"]);
}

#[test]
fn old_php_ternary_only_guards_its_then_side() {
    let then_side = "<?php\n$f = PHP_VERSION_ID < 70000 ? create_function('$a', 'return $a;') : function ($a) { return $a; };\n";
    let else_side = "<?php\n$f = PHP_VERSION_ID < 70000 ? function ($a) { return $a; } : create_function('$a', 'return $a;');\n";
    let fx = Fixture::new("ternold").file("t.php", then_side).file("e.php", else_side);
    let mut fs = vec![pfm("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "t.php", 2, CREATE_FUNCTION_MSG), pfm("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "e.php", 2, CREATE_FUNCTION_MSG)];
    full(fx.path(), &mut fs);
    assert_eq!(fs.iter().map(|f| (f.file.as_str(), f.severity.as_str())).collect::<Vec<_>>(), vec![("t.php", "info"), ("e.php", "error")]);
}

#[test]
fn ternary_parts_finds_then_and_else() {
    let t = "version_compare(php_version, '8.4', '>=') ? e_all : e_all & ~e_strict; $x = 1;";
    let (a, b) = detect::ternary_parts(t, 0).unwrap();
    assert_eq!(&t[a], " e_all ");
    assert_eq!(&t[b], " e_all & ~e_strict");
    let t2 = "(php_version_id >= 80000) ? a::b(c ? d : e) : ['x' => '?:'];";
    let (a, b) = detect::ternary_parts(t2, 1).unwrap();
    assert_eq!(&t2[a], " a::b(c ? d : e) ");
    assert_eq!(&t2[b], " ['x' => '?:']");
    for no in ["php_version_id >= 80000) { a(); }", "php_version_id >= 80000 ?: b;", "php_version_id >= 1, $a ? b : c);", "php_version_id >= 1 and $a ? b : c;"] {
        assert!(detect::ternary_parts(no, 0).is_none(), "{no}");
    }
}

#[test]
fn n16_throwable_on_an_exception_subclass_is_valid() {
    let fx = Fixture::new("n16")
        .file("src/InvalidDataException.php", "<?php\nnamespace Acme\\Exceptions;\nuse InvalidArgumentException;\nuse Throwable;\nfinal class InvalidDataException extends InvalidArgumentException implements Throwable {}\n")
        .file("src/Multi.php", "<?php\nclass NotFound extends \\RuntimeException\n    implements \\JsonSerializable, Throwable\n{\n}\n")
        .file("src/Bad.php", "<?php\nclass Bad implements Throwable {}\n");
    let r = "Interfaces.InternalInterfaces.throwableFound";
    let mut fs = vec![pf(r, "error", "src/InvalidDataException.php", 5), pf(r, "error", "src/Multi.php", 3), pf(r, "error", "src/Bad.php", 2)];
    full(fx.path(), &mut fs);
    assert_eq!(fs.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(), vec!["src/Bad.php"], "only a class that extends nothing fails");
    assert_eq!(fs[0].severity, "error");
}

#[test]
fn n17_strnatcmp_phpversion_is_a_version_check() {
    let src = "<?php\nif ( strnatcmp( phpversion(), '7.0.0' ) >= 0 ) {\n    add_filter( 'wp_default_editor', function() { return 'tmce'; } );\n} else {\n    add_filter( 'wp_default_editor', create_function( '', 'return \"tmce\";' ) );\n}\n";
    let fx = Fixture::new("n17").file("plugin.php", src);
    let mut fs = vec![pfm("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "plugin.php", 5, CREATE_FUNCTION_MSG)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    assert_eq!(verdict_of(&fs), "compatible");
    // The new-PHP side of the same check stays an error.
    let src2 = "<?php\nif ( strnatcmp( phpversion(), '7.0.0' ) >= 0 ) {\n    add_filter( 'x', create_function( '', 'return 1;' ) );\n}\n";
    let fx2 = Fixture::new("n17b").file("plugin.php", src2);
    let mut fs2 = vec![pfm("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "plugin.php", 3, CREATE_FUNCTION_MSG)];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "error");
}

#[test]
fn n18_implode_with_constant_glue_and_array_literal_is_correct_order() {
    let src = "<?php\nnamespace Acme\\Api;\nclass Verifier {\n    public function canonical($method, $path, $x, $nonce) {\n        return implode(SignedRequest::CANONICAL_SEPARATOR, [\n            SignedRequest::CANONICAL_V2_PREFIX,\n            strtoupper($method),\n            $x === null ? '' : (string) $x,\n            $nonce,\n        ]);\n    }\n    public function b($a) { return join(self::SEP, array( $a, 'b' )); }\n}\n";
    let fx = Fixture::new("n18").file("plugin.php", src);
    let mut fs = vec![pfm(IMPLODE_RULE, "error", "plugin.php", 5, IMPLODE_MSG), pfm(IMPLODE_RULE, "error", "plugin.php", 12, IMPLODE_MSG)];
    full(fx.path(), &mut fs);
    assert!(fs.is_empty(), "{:?}", sev(&fs));
}

#[test]
fn c2_reversed_implode_with_array_first_still_breaks() {
    let src = "<?php\nfunction acme_list() { return implode( array('a', 'b'), ', ' ); }\nadd_action( 'init', 'acme_list' );\n";
    let fx = Fixture::new("c2").file("plugin.php", src);
    let mut fs = vec![pfm(IMPLODE_RULE, "error", "plugin.php", 2, IMPLODE_MSG)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
    assert_eq!(verdict_of(&fs), "breaks");
}

#[test]
fn n19_static_in_a_template_without_class_is_info() {
    let fx = Fixture::new("n19")
        .file("backend/Dialog.php", "<?php\nclass Dialog {\n    public static function render() { include __DIR__ . '/templates/dialog.php'; }\n    public static function renderTemplate($t, $v) { echo $t; }\n}\n")
        .file("backend/templates/dialog.php", "<div class=\"modal-body\">\n    <?php static::renderTemplate( '_modal_body', compact( 'gateway' ) ) ?>\n</div>\n")
        .file("inc/helpers.php", "<?php\nclass Helper {}\nfunction h() { return static::x(); }\n");
    let r = "Classes.NewLateStaticBinding.OutsideClassScope";
    let mut fs = vec![pf(r, "error", "backend/templates/dialog.php", 2), pf(r, "error", "inc/helpers.php", 3)];
    full(fx.path(), &mut fs);
    assert_eq!(sev(&fs).iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), vec!["info", "error"], "a file that declares a class is not a template");
    assert!(fs[0].message.contains("template included from a class method"), "{}", fs[0].message);
}

#[test]
fn n20_argument_functions_changed_is_info() {
    let src = "<?php\nfunction get_wishlist( $id ) {\n    if ( is_array( $id ) ) { $id = $id[0]; }\n    error_log( print_r( func_get_args(), true ) );\n    return $id;\n}\n";
    let fx = Fixture::new("n20").file("plugin.php", src);
    let mut fs = vec![pf("FunctionUse.ArgumentFunctionsReportCurrentValue.Changed", "warning", "plugin.php", 4)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    assert_eq!(verdict_of(&fs), "compatible");
}

#[test]
fn n21_removed_ini_directives_are_info_but_recent_changes_stay() {
    let src = "<?php\nif ( ini_get( 'safe_mode' ) ) { echo 'safe mode on'; }\n$mb = function_exists( 'mb_strlen' ) && ( (int) ini_get( 'mbstring.func_overload' ) & 2 );\nini_set( 'auto_detect_line_endings', '1' );\nini_get( 'oci8.events' );\n";
    let fx = Fixture::new("n21").file("plugin.php", src);
    let ini = "IniDirectives.RemovedIniDirectives.";
    let mut fs = vec![
        pfm(&format!("{ini}safe_modeDeprecatedRemoved"), "error", "plugin.php", 2, "INI directive 'safe_mode' is deprecated since PHP 5.3 and removed since PHP 5.4"),
        pfm(&format!("{ini}mbstring_func_overloadDeprecatedRemoved"), "error", "plugin.php", 3, "INI directive 'mbstring.func_overload' is deprecated since PHP 7.2 and removed since PHP 8.0"),
    ];
    full(fx.path(), &mut fs);
    assert!(fs.iter().all(|f| f.severity == "info"), "{:?}", sev(&fs));
    assert_eq!(verdict_of(&fs), "compatible");
    // Deprecated only (still there), or removed in a checked version: kept as warnings.
    let mut fs2 = vec![
        pfm(&format!("{ini}auto_detect_line_endingsDeprecated"), "warning", "plugin.php", 4, "INI directive 'auto_detect_line_endings' is deprecated since PHP 8.1"),
        pfm(&format!("{ini}oci8_eventsRemoved"), "error", "plugin.php", 5, "INI directive 'oci8.events' is removed since PHP 8.4"),
    ];
    full(fx.path(), &mut fs2);
    assert_eq!(sev(&fs2).iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), vec!["warning", "warning"]);
}

#[test]
fn n22_asp_tag_in_a_js_template_is_info() {
    let src = "<script type=\"text/template\" id=\"preview\">\n    <div class=\"<%= settings.isLaunch ? 'active' : '' %>\"><%- label %></div>\n</script>\n";
    let fx = Fixture::new("n22").file("views/preview.php", src);
    let mut fs = vec![pf("Miscellaneous.RemovedAlternativePHPTags.MaybeASPOpenTagFound", "warning", "views/preview.php", 2)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    assert_eq!(verdict_of(&fs), "compatible");
}

#[test]
fn n23_function_exists_of_the_new_core_alternative_guards_the_else_branch() {
    let src = "<?php\nfunction set_ssl($resource, array $options) {\n    return function_exists('stream_context_set_options')\n        ? stream_context_set_options($resource, ['ssl' => $options])\n        : stream_context_set_option($resource, ['ssl' => $options]);\n}\n";
    let r = "FunctionUse.OptionalToRequiredFunctionParameters.stream_context_set_option_";
    let msg = |p: &str| format!("The \"{p}\" parameter for function stream_context_set_option() is missing. Passing this parameter is no longer optional. The optional nature of the parameter is deprecated since PHP 8.4");
    let mk = |file: &str, line: u32| vec![pfm(&format!("{r}option_nameSoftRequired"), "warning", file, line, &msg("option_name")), pfm(&format!("{r}valueSoftRequired"), "warning", file, line, &msg("value"))];
    let fx = Fixture::new("n23")
        .file("plugin.php", src)
        // Then-branch, and an if/else form.
        .file("then.php", "<?php\n$r = function_exists('stream_context_set_options') ? stream_context_set_option($c, ['ssl' => $o]) : null;\n")
        .file("ifelse.php", "<?php\nif (\\function_exists('stream_context_set_options')) {\n    stream_context_set_options($c, $o);\n} else {\n    stream_context_set_option($c, $o);\n}\n");
    let mut fs = mk("plugin.php", 5);
    fs.extend(mk("then.php", 2));
    fs.extend(mk("ifelse.php", 5));
    full(fx.path(), &mut fs);
    let got: Vec<(&str, &str)> = fs.iter().map(|f| (f.file.as_str(), f.severity.as_str())).collect();
    assert_eq!(got, vec![("plugin.php", "info"), ("plugin.php", "info"), ("then.php", "warning"), ("then.php", "warning"), ("ifelse.php", "info"), ("ifelse.php", "info")]);
    // An extension function is no version check: mbstring can be missing on any PHP.
    let fx2 = Fixture::new("n23b").file("p.php", "<?php\n$s = function_exists('mb_convert_encoding') ? mb_convert_encoding($s, 'UTF-8', 'ISO-8859-1') : utf8_encode($s);\n");
    let mut fs2 = vec![pfm("FunctionUse.RemovedFunctions.utf8_encodeDeprecated", "warning", "p.php", 2, "Function utf8_encode() is deprecated since PHP 8.2")];
    full(fx2.path(), &mut fs2);
    assert_eq!(fs2[0].severity, "warning");
}

#[test]
fn n24_escaped_name_in_function_exists_is_a_guard() {
    let src = "<?php\nnamespace Acme\\Polyfill;\nfunction array_first(array $array) {\n    if (\\function_exists('\\\\array_first')) {\n        return \\array_first($array);\n    }\n    foreach ($array as $v) { return $v; }\n    return null;\n}\n";
    let fx = Fixture::new("n24").file("src/Polyfill.php", src);
    let mut fs = vec![pfm("FunctionUse.NewFunctions.array_firstFound", "error", "src/Polyfill.php", 5, "The function array_first() is not present in PHP version 8.4 or earlier")];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info", "{}", fs[0].message);
    assert_eq!(verdict_of(&fs), "compatible");
}

/// A rules folder holding only the given not-reached.json.
fn not_reached_rules(json: &str) -> (Fixture, detect::Rules) {
    let fx = Fixture::new("nrrules").file("not-reached.json", json);
    let r = detect::Rules::load(fx.path()).unwrap();
    (fx, r)
}

#[test]
fn n25_n27_hand_checked_never_reached_errors_are_info_for_that_version_only() {
    let n25 = "<?php\nfunction acme_clean( $s ) {\n    if ( is_string( $s ) ) { return trim( $s ); }\n    if ( is_array( $s ) ) {\n        reset( $s );\n        while ( list( $k, $v ) = each( $s ) ) { $s[ $k ] = acme_clean( $v ); }\n    }\n    return $s;\n}\n$ua = acme_clean( $_SERVER['HTTP_USER_AGENT'] ?? '' );\n";
    let n27 = "<?php\nfunction acme_get_features() { return array('a', 'b'); }\nfunction acme_the_features() { echo implode( acme_get_features(), ', ' ); }\n";
    let fx = Fixture::new("n25").file("plugin.php", n25).file("features.php", n27);
    let (_r, rules) = not_reached_rules(&format!(
        r#"[{{"slug":"acme","version":"1.0","file":"plugin.php","rule_id":"{P}FunctionUse.RemovedFunctions.eachDeprecatedRemoved","reason":"only callers pass strings","reviewed_by_date":"2026-11-19"}},
            {{"slug":"acme","version":"1.0","file":"features.php","rule_id":"{P}{IMPLODE_RULE}","reason":"function never called","reviewed_by_date":"2026-11-19"}}]"#
    ));
    assert!(rules.summary().contains("not_reached=2"), "{}", rules.summary());
    let fresh = || vec![pfm("FunctionUse.RemovedFunctions.eachDeprecatedRemoved", "error", "plugin.php", 6, EACH_MSG), pfm(IMPLODE_RULE, "error", "features.php", 3, IMPLODE_MSG)];
    let mut fs = fresh();
    full(fx.path(), &mut fs);
    assert_eq!(verdict_of(&fs), "breaks", "the scan alone cannot prove this");
    rules.apply_not_reached("acme", "1.0", &mut fs);
    assert!(fs.iter().all(|f| f.severity == "info"), "{:?}", sev(&fs));
    assert!(fs[0].message.starts_with("Checked by hand, this code is not reached (only callers pass strings): Function each()"), "{}", fs[0].message);
    assert_eq!(verdict_of(&fs), "compatible");
    // A new version, another package, file or rule is checked again.
    for (pkg, ver) in [("acme", "1.1"), ("other", "1.0")] {
        let mut fs = fresh();
        full(fx.path(), &mut fs);
        rules.apply_not_reached(pkg, ver, &mut fs);
        assert_eq!(verdict_of(&fs), "breaks", "{pkg}@{ver}");
    }
    let mut fs = vec![pfm("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "plugin.php", 6, CREATE_FUNCTION_MSG)];
    rules.apply_not_reached("acme", "1.0", &mut fs);
    assert_eq!(fs[0].severity, "error");
}

#[test]
fn shipped_not_reached_entries_load_and_match_exact_versions() {
    let rules = rules();
    assert!(rules.summary().contains("not_reached=10"), "{}", rules.summary());
    let cases = [
        ("wp-db-backup", "2.5.3", "wp-db-backup.php", "FunctionUse.RemovedFunctions.eachDeprecatedRemoved"),
        ("contact-form-plugin", "4.4.1", "contact_form.php", "FunctionUse.RemovedFunctions.eachDeprecatedRemoved"),
        ("wptouch", "4.3.62", "core/admin-extensions.php", IMPLODE_RULE),
        ("wptouch", "4.3.62", "core/admin-themes.php", IMPLODE_RULE),
        ("backuply", "1.6.0", "lib/ftp.php", "FunctionUse.RemovedFunctions.eachDeprecatedRemoved"),
        ("matomo", "5.13.1", "app/libs/HTML/QuickForm2/Renderer/Default.php", "FunctionUse.RemovedFunctions.eachDeprecatedRemoved"),
        ("matomo", "5.13.1", "app/vendor/lox/xhprof/xhprof_lib/utils/xhprof_runs.php", "FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved"),
        ("maxbuttons", "9.8.6", "classes/buttons.php", "ParameterValues.NewExitAsFunctionCall.TypeError"),
        ("all-in-one-favicon", "4.8", "includes/aio-favicon-backend.php", "FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved"),
        ("wordpress-importer", "0.9.7", "parsers/class-wxr-parser-simplexml.php", "FunctionUse.RemovedFunctions.libxml_disable_entity_loaderDeprecated"),
    ];
    for (slug, ver, file, rule) in cases {
        let mut fs = vec![pf(rule, "error", file, 1)];
        rules.apply_not_reached(slug, ver, &mut fs);
        assert_eq!(fs[0].severity, "info", "{slug} {file}");
        let mut fs = vec![pf(rule, "error", file, 1)];
        rules.apply_not_reached(slug, "999.0", &mut fs);
        assert_eq!(fs[0].severity, "error", "{slug}: a new version is checked again");
    }
}

#[test]
fn n26_bundled_ftp_class_is_a_component() {
    let fx = Fixture::new("n26")
        .file("plugin.php", "<?php\nrequire_once __DIR__ . '/lib/ftp.php';\n$f = new ftp_base();\n")
        .file("lib/ftp.php", "<?php\nclass ftp_base {\n    function glob_regexp($pattern, $probe) { return true; }\n    function glob($pattern, $handle = null) {\n        $output = array();\n        $list = array('a' => 1);\n        while (list($k, $v) = each($list)) { $output[] = $k; }\n        return $output;\n    }\n}\n")
        // Same file name, not the FTP class: no component.
        .file("other/ftp.php", "<?php\nfunction my_ftp() {}\n");
    let comps = detect::dedupe_components(detect::components(fx.path(), &rules()));
    assert_eq!(comps.iter().map(|c| format!("{} {}", c.name, c.path)).collect::<Vec<_>>(), vec!["pemftp lib"]);
    let mut fs = vec![pfm("FunctionUse.RemovedFunctions.eachDeprecatedRemoved", "error", "lib/ftp.php", 7, EACH_MSG)];
    for f in fs.iter_mut() {
        f.component = detect::attribute(&f.file, &comps);
    }
    full(fx.path(), &mut fs);
    let got = stored(&[("8.4".to_string(), fs)], 0);
    assert_eq!(got[0].1, "issues");
    assert!(got[0].3[0].starts_with(crate::BUNDLED_PREFIX), "{}", got[0].3[0]);
}

#[test]
fn h1_signatures_for_quickform2_xhprof_and_phpmyadmin() {
    let fx = Fixture::new("h1sigs")
        .file("app/libs/HTML/QuickForm2.php", "<?php\n    class HTML_QuickForm2 extends \\HTML_QuickForm2_Container\n    {\n    }\n")
        .file("app/libs/HTML/QuickForm2/Renderer/Default.php", "<?php\n")
        .file("app/vendor/lox/xhprof/xhprof_lib/utils/xhprof_runs.php", "<?php\n    class XHProfRuns_Default implements \\iXHProfRuns\n    {\n    }\n")
        .file("lib/phpMyAdmin/libraries/classes/Version.php", "<?php\n\ndeclare(strict_types=1);\n\nnamespace PhpMyAdmin;\n\nfinal class Version\n{\n    public const VERSION = '5.2.3' . VERSION_SUFFIX;\n}\n")
        .file("lib/phpMyAdmin/libraries/classes/Encoding.php", "<?php\n")
        // A plugin's own Version.php in another namespace is not phpMyAdmin.
        .file("src/libraries/classes/Version.php", "<?php\nnamespace Acme;\nclass Version { const VERSION = '1.0'; }\n");
    let comps = detect::dedupe_components(detect::components(fx.path(), &rules()));
    let got: Vec<String> = comps.iter().map(|c| format!("{} {} {}", c.name, c.version.as_deref().unwrap_or("-"), c.path)).collect();
    assert_eq!(got, vec!["html_quickform2 - app/libs/HTML", "phpmyadmin 5.2.3 lib/phpMyAdmin", "xhprof - app/vendor/lox/xhprof"]);
    assert_eq!(detect::attribute("app/libs/HTML/QuickForm2/Renderer/Default.php", &comps).as_deref(), Some("html_quickform2"));
    assert_eq!(detect::attribute("app/vendor/lox/xhprof/xhprof_lib/utils/xhprof_runs.php", &comps).as_deref(), Some("xhprof"));
    assert_eq!(detect::attribute("lib/phpMyAdmin/libraries/classes/Encoding.php", &comps).as_deref(), Some("phpmyadmin@5.2.3"));
}

#[test]
fn g2_phpmyadmin_engine_selected_calls_are_info() {
    let fx = Fixture::new("g2").file("lib/phpMyAdmin/libraries/classes/Encoding.php", "<?php\n").file("lib/phpMyAdmin/vendor/phpmyadmin/shapefile/src/ShapeFile.php", "<?php\n");
    let mut fs = vec![
        pfm("FunctionUse.RemovedFunctions.recode_stringRemoved", "error", "lib/phpMyAdmin/libraries/classes/Encoding.php", 196, "Function recode_string() is removed since PHP 7.4"),
        pfm("FunctionUse.RemovedFunctions.dbase_openRemoved", "error", "lib/phpMyAdmin/vendor/phpmyadmin/shapefile/src/ShapeFile.php", 628, "Function dbase_open() is removed since PHP 5.3"),
        pfm("FunctionUse.RemovedFunctions.recode_stringRemoved", "error", "inc/mine.php", 3, "Function recode_string() is removed since PHP 7.4"),
    ];
    full(fx.path(), &mut fs);
    assert_eq!(sev(&fs).iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), vec!["info", "info", "error"]);
}

#[test]
fn m4_bare_parent_word_is_info_but_parent_call_stays() {
    let src = "<?php\nclass Lib {\n    function w($c) { return (is_array($c) || parent.isObject($c)) ? json_encode($c) : $c; }\n    function x() { return parent::x(); }\n}\n";
    let fx = Fixture::new("m4").file("library.php", src);
    let r = "Classes.RemovedOrphanedParent.Removed";
    let msg = "Using \"parent\" inside a class without parent is deprecated since PHP 7.4 and removed since PHP 8.0";
    let mut fs = vec![pfm(r, "error", "library.php", 3, msg), pfm(r, "error", "library.php", 4, msg)];
    full(fx.path(), &mut fs);
    assert_eq!(sev(&fs).iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), vec!["info", "error"]);
}

#[test]
fn suppression_patterns_and_message_regex() {
    let fx = Fixture::new("supp2").file(
        "suppressions.json",
        r#"[{"rule_id":"R.*Removed","path_regex":".*","message_regex":"since PHP [0-7]\\.","action":"info","reason":"old"},{"rule_id":"R.*","path_regex":".*","action":"warning","reason":"rest"}]"#,
    );
    let mut fs = vec![
        pfm("x", "error", "a.php", 1, "removed since PHP 5.4"),
        pfm("x", "error", "a.php", 2, "removed since PHP 8.4"),
        pfm("x", "error", "a.php", 3, "deprecated since PHP 5.4"),
    ];
    fs[0].rule_id = "R.aDeprecatedRemoved".into();
    fs[1].rule_id = "R.bRemoved".into();
    fs[2].rule_id = "R.cDeprecated".into();
    detect::apply_suppressions(fx.path(), &mut fs);
    assert_eq!(fs.iter().map(|f| f.severity.as_str()).collect::<Vec<_>>(), vec!["info", "warning", "warning"]);
    assert_eq!(fs[0].message, "removed since PHP 5.4 (old)");
}

#[test]
fn wordpress_api_download_link_is_used_for_every_version() {
    let v = serde_json::json!({
        "name": "WP Umbrella &#8211; Update", "version": "2.28.1", "active_installs": 90000,
        "download_link": "https://downloads.wordpress.org/plugin/wp-health.v2.28.1.zip",
        "versions": {"v2.9.4": "https://downloads.wordpress.org/plugin/wp-health.v2.9.4.zip", "trunk": "https://downloads.wordpress.org/plugin/wp-health.zip", "2.0": "https://downloads.wordpress.org/plugin/wp-health.2.0.zip"},
        "tags": {"a": "backup"}
    });
    let info = crate::parse_wp_info(&v).unwrap();
    assert_eq!(info.display_name.as_deref(), Some("WP Umbrella \u{2013} Update"));
    assert_eq!(info.zip_url(None).unwrap(), "https://downloads.wordpress.org/plugin/wp-health.v2.28.1.zip");
    assert_eq!(info.zip_url(Some("2.28.1")).unwrap(), "https://downloads.wordpress.org/plugin/wp-health.v2.28.1.zip", "not {{slug}}.{{version}}.zip");
    assert_eq!(info.zip_url(Some("2.0")).unwrap(), "https://downloads.wordpress.org/plugin/wp-health.2.0.zip");
    assert!(info.zip_url(Some("1.9")).is_err(), "unpublished version");
    // Stable tag "trunk": the current version is only published as {slug}.zip.
    let trunk = crate::parse_wp_info(&serde_json::json!({"version": "5.2.3.03", "download_link": "https://downloads.wordpress.org/plugin/wp-phpmyadmin-extension.zip", "versions": []})).unwrap();
    assert_eq!(trunk.zip_url(Some("5.2.3.03")).unwrap(), "https://downloads.wordpress.org/plugin/wp-phpmyadmin-extension.zip");
    let evil = crate::parse_wp_info(&serde_json::json!({"version": "1", "download_link": "https://example.com/x.zip"})).unwrap();
    assert!(evil.zip_url(None).is_err());
    assert!(crate::parse_wp_info(&serde_json::json!({"error": "Plugin not found."})).is_err());
}

#[test]
fn scan_dir_metadata_is_stored_and_exported() {
    let fx = Fixture::new("meta");
    let conn = crate::db::open(fx.path().join("t.sqlite").to_str().unwrap()).unwrap();
    let eco = "wordpress-plugin";
    crate::store_dir_meta(&conn, eco, "wp-file-manager", Some("File Manager &amp; More"), Some(1_000_000)).unwrap();
    // Flags not given keep the stored values; nothing given is a no-op.
    crate::store_dir_meta(&conn, eco, "wp-file-manager", None, Some(900_000)).unwrap();
    crate::store_dir_meta(&conn, eco, "wp-file-manager", None, None).unwrap();
    assert!(crate::store_dir_meta(&conn, eco, "x", None, Some(-1)).is_err());
    let run = crate::trace::Run::start(&conn, "test", serde_json::json!({})).unwrap();
    crate::store(&conn, &run.id, eco, "wp-file-manager", "8.0.7", &[], &[], &[("8.4".to_string(), vec![])], 0, &|_| false).unwrap();
    let out = fx.path().join("out");
    crate::export(&conn, &out).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("v1/wordpress-plugin/wp-file-manager/8.0.7.json")).unwrap()).unwrap();
    assert_eq!(doc["display_name"], "File Manager & More");
    assert_eq!(doc["active_installs"], 900_000);
}

// ---------- live-data review, round 4 ----------
//
// Real code patterns from scanned plugins; rule IDs and messages as phpcs reports them for PHP 8.4.

const RESOURCE_MSG: &str = "'resource' is a soft reserved keyword as of PHP version 7.0 and should not be used to name a class, interface, trait or enum or as part of a namespace (class declaration)";
const NUMERIC_MSG: &str = "'numeric' is a soft reserved keyword as of PHP version 7.0 and should not be used to name a class, interface, trait or enum or as part of a namespace (class declaration)";
const ENUM_MSG: &str = "'enum' is a soft reserved keyword as of PHP version 8.1 and should not be used to name a class, interface, trait or enum or as part of a namespace (class declaration)";
const DUNDER_MSG: &str = "Function name \"__pl\" is discouraged; PHP has reserved all function names with a double underscore prefix for future use.";
const INTEGER_MSG: &str = "'integer' is not a valid parameter type declaration. Did you mean int ?";
const STRIP_TAGS_MSG: &str = "When passing strip_tags() the $allowed_tags parameter as an array, the tags should not be enclosed in <> brackets. Found: array('<a>')";
const INI_URL_INCLUDE_MSG: &str = "INI directive 'allow_url_include' is deprecated since PHP 7.4";
const INI_LINE_ENDINGS_MSG: &str = "INI directive 'auto_detect_line_endings' is deprecated since PHP 8.1";
const USER_ERROR_RULE: &str = "ParameterValues.RemovedTriggerErrorLevel.Deprecated";
const USER_ERROR_MSG: &str = "Passing E_USER_ERROR to trigger_error() is deprecated since 8.4. Throw an exception or call exit with a string message instead.";
const LIBXML_RULE: &str = "FunctionUse.RemovedFunctions.libxml_disable_entity_loaderDeprecated";
const LIBXML_MSG: &str = "Function libxml_disable_entity_loader() is deprecated since PHP 8.0";

#[test]
fn r4_f1_names_that_never_change_on_upgrade_are_info() {
    // php-mcp-schema (premium-addons, formidable, mcp-adapter), admin-menu-editor, pagelayer,
    // phpseclib (hummingbird, duplicator), wp-security-audit-log.
    let src = "<?php\nnamespace Mcp\\Schema;\nclass Resource extends Schema {}\nclass Numeric {}\nclass Enum extends Schema {}\nfunction __pl( $s ) { return $s; }\nfunction f( Integer $k1 ) { return 1; }\n$s = strip_tags( $s, array( '<a>' ) );\n";
    let fx = Fixture::new("r4f1").file("a.php", src);
    let mut fs = vec![
        pfm("Keywords.ForbiddenNames.resourceFound", "warning", "a.php", 3, RESOURCE_MSG),
        pfm("Keywords.ForbiddenNames.numericFound", "warning", "a.php", 4, NUMERIC_MSG),
        pfm("Keywords.ForbiddenNames.enumFound", "warning", "a.php", 5, ENUM_MSG),
        pfm("FunctionNameRestrictions.ReservedFunctionNames.FunctionDoubleUnderscore", "warning", "a.php", 6, DUNDER_MSG),
        pfm("FunctionDeclarations.NewParamTypeDeclarations.InvalidLongTypeFound", "warning", "a.php", 7, INTEGER_MSG),
        pfm("ParameterValues.NewStripTagsAllowableTagsArray.Invalid", "warning", "a.php", 8, STRIP_TAGS_MSG),
    ];
    full(fx.path(), &mut fs);
    assert!(fs.iter().all(|f| f.severity == "info"), "{:?}", sev(&fs));
    assert_eq!(verdict_of(&fs), "compatible");
    // Narrow: a name that is also a hard reserved keyword, and other messages of the same rules, stay.
    let mut fs = vec![
        pfm("Keywords.ForbiddenNames.enumFound", "error", "a.php", 5, "'enum' is a soft reserved keyword as of PHP version 8.1 and a reserved keyword as of PHP version 8.1 and should not be used to name a class, interface, trait or enum or as part of a namespace (class declaration)"),
        pfm("FunctionDeclarations.NewParamTypeDeclarations.InvalidLongTypeFound", "warning", "a.php", 7, "'boolean' is not a valid parameter type declaration. Did you mean bool ?"),
        pfm("ParameterValues.NewStripTagsAllowableTagsArray.Found", "error", "a.php", 8, "The strip_tags() function did not accept $allowed_tags to be passed in array format in PHP 7.3 and earlier."),
    ];
    full(fx.path(), &mut fs);
    assert_eq!(fs.iter().map(|f| f.severity.as_str()).collect::<Vec<_>>(), vec!["error", "warning", "error"]);
}

#[test]
fn r4_f2_ini_get_of_a_deprecated_directive_is_info_but_ini_set_stays() {
    // wp-reviews-plugin-for-google, hide-my-wp, anti-spam; tablepress/https-redirection set it.
    let src = "<?php\nif ( ini_get( 'allow_url_include' ) ) {\n\t$warn = true;\n}\n$on = (bool) ini_get('allow_url_include');\n@ini_set('auto_detect_line_endings', true);\nini_set( 'allow_url_include', ini_get( 'allow_url_include' ) );\n";
    let fx = Fixture::new("r4f2").file("a.php", src);
    let mut fs = vec![
        pfm("IniDirectives.RemovedIniDirectives.allow_url_includeDeprecated", "warning", "a.php", 2, INI_URL_INCLUDE_MSG),
        pfm("IniDirectives.RemovedIniDirectives.allow_url_includeDeprecated", "warning", "a.php", 5, INI_URL_INCLUDE_MSG),
    ];
    full(fx.path(), &mut fs);
    assert!(fs.iter().all(|f| f.severity == "info"), "{:?}", sev(&fs));
    assert_eq!(verdict_of(&fs), "compatible");
    let mut fs = vec![
        pfm("IniDirectives.RemovedIniDirectives.auto_detect_line_endingsDeprecated", "warning", "a.php", 6, INI_LINE_ENDINGS_MSG),
        pfm("IniDirectives.RemovedIniDirectives.allow_url_includeDeprecated", "warning", "a.php", 7, INI_URL_INCLUDE_MSG),
    ];
    full(fx.path(), &mut fs);
    assert_eq!(fs.iter().map(|f| f.severity.as_str()).collect::<Vec<_>>(), vec!["warning", "warning"]);
    assert_eq!(verdict_of(&fs), "issues");
}

const PSR7_TO_STRING: &str = "<?php\nnamespace GuzzleHttp\\Psr7;\nclass Stream {\n    public function __toString(): string\n    {\n        try {\n            if ($this->isSeekable()) {\n                $this->seek(0);\n            }\n            return $this->getContents();\n        } catch (\\Throwable $e) {\n            if (\\PHP_VERSION_ID >= 70400) {\n                throw $e;\n            }\n            \\trigger_error(\\sprintf('%s::__toString exception: %s', self::class, (string) $e), \\E_USER_ERROR);\n            return '';\n        }\n    }\n}\n";

#[test]
fn r4_f3_rethrow_after_new_php_check_is_an_early_exit() {
    // guzzle psr7 Stream/PumpStream/StreamDecoratorTrait (wordpress-seo, wp-smushit, duplicator).
    let line = line_of(PSR7_TO_STRING, "trigger_error");
    let fx = Fixture::new("r4f3").file("vendor/guzzlehttp/psr7/src/Stream.php", PSR7_TO_STRING);
    let mut fs = vec![pfm(USER_ERROR_RULE, "warning", "vendor/guzzlehttp/psr7/src/Stream.php", line, USER_ERROR_MSG)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info", "{}", fs[0].message);
    assert_eq!(verdict_of(&fs), "compatible");
    // Braceless form.
    let braceless = PSR7_TO_STRING.replace("if (\\PHP_VERSION_ID >= 70400) {\n                throw $e;\n            }", "if (\\PHP_VERSION_ID >= 70400) throw $e;");
    let fx = Fixture::new("r4f3b").file("s.php", &braceless);
    let mut fs = vec![pfm(USER_ERROR_RULE, "warning", "s.php", line_of(&braceless, "trigger_error"), USER_ERROR_MSG)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    // Not an early exit: throwing a new exception on new PHP (the code refuses to run there),
    // an old-PHP check, and a throw after a negated feature check (fails on new PHP).
    for (from, to) in [
        ("throw $e;", "throw new \\RuntimeException('unsupported');"),
        (">= 70400", "< 70400"),
    ] {
        let s = PSR7_TO_STRING.replace(from, to);
        let fx = Fixture::new("r4f3n").file("s.php", &s);
        let mut fs = vec![pfm(USER_ERROR_RULE, "warning", "s.php", line_of(&s, "trigger_error"), USER_ERROR_MSG)];
        full(fx.path(), &mut fs);
        assert_eq!(fs[0].severity, "warning", "{to}");
    }
    let neg = "<?php\nfunction f($e) {\n    if ( ! function_exists( 'create_function' ) ) {\n        throw $e;\n    }\n    return create_function( '', 'return 1;' );\n}\n";
    let fx = Fixture::new("r4f3neg").file("n.php", neg);
    let mut fs = vec![pfm("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "n.php", 6, CREATE_FUNCTION_MSG)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "error");
}

#[test]
fn r4_f4_guard_held_in_a_variable() {
    // astra-sites st-wxr-importer.php and pojo-accessibility svg-sanitizer.php.
    let astra = "<?php\nclass Importer {\n\tpublic function sanitize( $content ) {\n\t\t// If the server's PHP version is 8 or up, make sure to disable the ability to load external entities.\n\t\t$php_version_under_eight = version_compare( PHP_VERSION, '8.0.0', '<' );\n\t\tif ( $php_version_under_eight ) {\n\t\t\t// phpcs:disable Generic.PHP.DeprecatedFunctions.Deprecated\n\t\t\t$libxml_disable_entity_loader = libxml_disable_entity_loader( true );\n\t\t\t// phpcs:enable Generic.PHP.DeprecatedFunctions.Deprecated\n\t\t}\n\t\t$libxml_use_internal_errors = libxml_use_internal_errors( true );\n\t\t$sanitized = $dom->saveXML( $dom->documentElement, LIBXML_NOEMPTYTAG );\n\n\t\t// Restore defaults.\n\t\tif ( $php_version_under_eight && isset( $libxml_disable_entity_loader ) ) {\n\t\t\tlibxml_disable_entity_loader( $libxml_disable_entity_loader );\n\t\t}\n\t\treturn $sanitized;\n\t}\n}\n";
    // sabre/xml Reader.php (woocommerce-pdf-invoices-packing-slips).
    let sabre = "<?php\nclass Reader extends \\XMLReader {\n    public function parse(): array\n    {\n        $previousEntityState = null;\n        $shouldCallLibxmlDisableEntityLoader = (\\LIBXML_VERSION < 20900);\n        if ($shouldCallLibxmlDisableEntityLoader) {\n            $previousEntityState = libxml_disable_entity_loader(true);\n        }\n        try {\n            $result = $this->parseCurrentElement();\n        } finally {\n            if ($shouldCallLibxmlDisableEntityLoader) {\n                libxml_disable_entity_loader($previousEntityState);\n            }\n        }\n        return $result;\n    }\n}\n";
    // jetpack class-jetpack-podcast-feed-locator.php.
    let jetpack = "<?php\nclass Locator {\n\tprivate function safely_load_xml( $xml ) {\n\t\t$disable_entity_loader = PHP_VERSION_ID < 80000;\n\n\t\tif ( $disable_entity_loader ) {\n\t\t\t// phpcs:ignore Generic.PHP.DeprecatedFunctions.Deprecated\n\t\t\t$loader = libxml_disable_entity_loader( true );\n\t\t}\n\t\t$return = new DOMDocument();\n\t\tif ( $disable_entity_loader && isset( $loader ) ) {\n\t\t\tlibxml_disable_entity_loader( $loader );\n\t\t}\n\t\treturn $return;\n\t}\n}\n";
    for (name, src) in [("astra", astra), ("sabre", sabre), ("jetpack", jetpack)] {
        let fx = Fixture::new("r4f4").file("a.php", src);
        let mut fs: Vec<Finding> = src.lines().enumerate().filter(|(_, l)| l.contains("libxml_disable_entity_loader(")).map(|(k, _)| pfm(LIBXML_RULE, "warning", "a.php", k as u32 + 1, LIBXML_MSG)).collect();
        assert_eq!(fs.len(), 2, "{name}");
        full(fx.path(), &mut fs);
        assert!(fs.iter().all(|f| f.severity == "info"), "{name}: {:?}", sev(&fs));
        assert_eq!(verdict_of(&fs), "compatible", "{name}");
    }
    // A feature check in a variable guards a removed function (errors only).
    let has = "<?php\n$has = function_exists( 'create_function' );\nif ( $has ) {\n\t$f = create_function( '', 'return 1;' );\n}\n";
    let fx = Fixture::new("r4f4has").file("a.php", has);
    let mut fs = vec![pfm("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "a.php", 4, CREATE_FUNCTION_MSG)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
    // Not guards: a new-PHP check, a combined expression, a negated variable, a variable
    // reassigned after the check, and a negated feature check.
    for bad in [
        "$v = version_compare( PHP_VERSION, '8.0.0', '>=' );\nif ( $v ) {",
        "$v = PHP_VERSION_ID < 80000 || $force;\nif ( $v ) {",
        "$v = PHP_VERSION_ID < 80000;\nif ( ! $v ) {",
        "$v = PHP_VERSION_ID < 80000;\n$v = true;\nif ( $v ) {",
        "$v = ! function_exists( 'libxml_disable_entity_loader' );\nif ( $v ) {",
        "$v = PHP_VERSION_ID >= 80000;\nif ( $v && $x ) {\n} else {",
    ] {
        let src = format!("<?php\n{bad}\n\t$l = libxml_disable_entity_loader( true );\n}}\n");
        let fx = Fixture::new("r4f4bad").file("a.php", &src);
        let mut fs = vec![pfm(LIBXML_RULE, "warning", "a.php", line_of(&src, "$l = libxml"), LIBXML_MSG)];
        full(fx.path(), &mut fs);
        assert_eq!(fs[0].severity, "warning", "{bad}");
    }
    // The else part of a new-PHP variable check runs only on old PHP.
    let src = "<?php\n$is_new = PHP_VERSION_ID >= 80000;\nif ( $is_new ) {\n\t$l = null;\n} else {\n\t$l = libxml_disable_entity_loader( true );\n}\n";
    let fx = Fixture::new("r4f4else").file("a.php", src);
    let mut fs = vec![pfm(LIBXML_RULE, "warning", "a.php", 6, LIBXML_MSG)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info");
}

#[test]
fn r4_f5_not_reached_also_applies_to_warnings() {
    // wordpress-importer parsers/class-wxr-parser-simplexml.php: null sentinel.
    let src = "<?php\n\t\t$dom       = new DOMDocument();\n\t\t$old_value = null;\n\t\tif ( function_exists( 'libxml_disable_entity_loader' ) && PHP_VERSION_ID < 80000 ) {\n\t\t\t$old_value = libxml_disable_entity_loader( true );\n\t\t}\n\t\t$success = $dom->loadXML( file_get_contents( $file ) );\n\t\tif ( ! is_null( $old_value ) ) {\n\t\t\tlibxml_disable_entity_loader( $old_value );\n\t\t}\n";
    let file = "parsers/class-wxr-parser-simplexml.php";
    let fx = Fixture::new("r4f5").file(file, src);
    let fresh = || vec![pfm(LIBXML_RULE, "warning", file, 5, LIBXML_MSG), pfm(LIBXML_RULE, "warning", file, 9, LIBXML_MSG)];
    let mut fs = fresh();
    full(fx.path(), &mut fs);
    assert_eq!(fs.iter().map(|f| f.severity.as_str()).collect::<Vec<_>>(), vec!["info", "warning"], "the scan alone cannot prove this");
    rules().apply_not_reached("wordpress-importer", "0.9.7", &mut fs);
    assert!(fs.iter().all(|f| f.severity == "info"), "{:?}", sev(&fs));
    assert!(fs[1].message.starts_with("Checked by hand, this code is not reached ("), "{}", fs[1].message);
    assert_eq!(verdict_of(&fs), "compatible");
    let mut fs = fresh();
    full(fx.path(), &mut fs);
    rules().apply_not_reached("wordpress-importer", "0.9.8", &mut fs);
    assert_eq!(verdict_of(&fs), "issues", "a new version is checked again");
}

#[test]
fn r4_f7_hand_checked_breaks_are_not_reached() {
    // maxbuttons classes/buttons.php: exit(true) after an exit that always runs.
    let maxbuttons = "<?php\nclass maxButtons {\n\tpublic static function ajax_action() {\n\t\t$redirect = add_query_arg('page', $page, $redirect);\n\n\t\tif (isset($redirect)) {\n\t\t\t$response = array('redirection' => $redirect);\n\t\t\techo json_encode($response);\n\t\t\texit();\n\t\t}\n\n\t\texit(true);\n\t}\n}\n";
    let fx = Fixture::new("r4f7").file("classes/buttons.php", maxbuttons);
    let mut fs = vec![
        pfm("ParameterValues.NewExitAsFunctionCall.TypeError", "error", "classes/buttons.php", 12, "Passing a boolean to exit() will result in a TypeError since PHP 8.4. Found: \"true\""),
        pfm("FunctionDeclarations.RemovedImplicitlyNullableParam.Deprecated", "warning", "assets/libraries/scssphp/src/Parser.php", 143, "Implicitly marking a parameter as nullable is deprecated since PHP 8.4."),
    ];
    full(fx.path(), &mut fs);
    assert_eq!(verdict_of(&fs), "breaks");
    rules().apply_not_reached("maxbuttons", "9.8.6", &mut fs);
    assert_eq!(verdict_of(&fs), "issues");
    // all-in-one-favicon: create_function only for a notice the settings page never sets.
    let mut fs = vec![pfm("FunctionUse.RemovedFunctions.create_functionDeprecatedRemoved", "error", "includes/aio-favicon-backend.php", 346, CREATE_FUNCTION_MSG)];
    rules().apply_not_reached("all-in-one-favicon", "4.8", &mut fs);
    assert_eq!(verdict_of(&fs), "compatible");
}

/// Runs the real analyzer when tools/php is installed (skipped otherwise).
fn real_phpcs(root: &Path) -> Option<Vec<Finding>> {
    use crate::model::Analyzer;
    let tools = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/php");
    let a = crate::php::PhpCompatibility::locate(&tools).ok()?;
    Some(a.analyze(root, "8.4").expect("phpcs runs"))
}

#[test]
fn r4_f6_phpcs_annotations_in_plugin_code_do_not_hide_findings() {
    // wp-all-export.php: `phpcs:ignoreFile` with codes still skips the whole file.
    let export = "<?php\n\n// phpcs:ignoreFile WordPress.NamingConventions.PrefixAllGlobals.NonPrefixedHooknameFound,WordPress.NamingConventions.PrefixAllGlobals.NonPrefixedFunctionFound -- legitimate plugin prefixes\ndefined( 'ABSPATH' ) || exit;\nclass PMXE_Plugin {\n\t\tpublic function activation() {\n\t\t\t// uncaught exception doesn't prevent plugin from being activated, therefore replace it with fatal error so it does\n            // phpcs:ignore WordPress.PHP.DevelopmentFunctions.error_log_trigger_error\n            set_exception_handler(function($e) {trigger_error($e->getMessage(), E_USER_ERROR); });\n\t\t}\n}\n";
    // google-analytics-for-wordpress site-notes/Controller.php: bare `phpcs:ignore`.
    let notes = "<?php\nclass Controller {\n\tpublic function export_notes() {\n\t\t$outstream = fopen( 'php://output', 'w' );\n\t\t// phpcs:ignore\n\t\tfputcsv( $outstream, $headers );\n\t\tforeach ( $rows as $row ) {\n\t\t\t// phpcs:ignore\n\t\t\tfputcsv( $outstream, $row );\n\t\t}\n\t}\n}\n";
    // jetpack: targeted ignores on code guarded by a variable stay guarded.
    let jetpack = "<?php\nclass Locator {\n\tprivate function safely_load_xml( $xml ) {\n\t\t$disable_entity_loader = PHP_VERSION_ID < 80000;\n\t\tif ( $disable_entity_loader ) {\n\t\t\t// phpcs:ignore Generic.PHP.DeprecatedFunctions.Deprecated, PHPCompatibility.FunctionUse.RemovedFunctions.libxml_disable_entity_loaderDeprecated\n\t\t\t$loader = libxml_disable_entity_loader( true );\n\t\t}\n\t\t$return = new DOMDocument();\n\t\tif ( $disable_entity_loader && isset( $loader ) ) {\n\t\t\t// phpcs:ignore Generic.PHP.DeprecatedFunctions.Deprecated, PHPCompatibility.FunctionUse.RemovedFunctions.libxml_disable_entity_loaderDeprecated\n\t\t\tlibxml_disable_entity_loader( $loader );\n\t\t}\n\t\treturn $return;\n\t}\n\tpublic function amp_host( $host ) {\n\t\tif ( function_exists( 'idn_to_utf8' ) ) {\n\t\t\t$variant = defined( 'INTL_IDNA_VARIANT_UTS46' )\n\t\t\t\t? INTL_IDNA_VARIANT_UTS46\n\t\t\t\t// phpcs:ignore PHPCompatibility.Constants.RemovedConstants.intl_idna_variant_2003DeprecatedRemoved\n\t\t\t\t: INTL_IDNA_VARIANT_2003;\n\t\t\t$host = idn_to_utf8( $host, IDNA_DEFAULT, $variant );\n\t\t}\n\t\treturn $host;\n\t}\n\tpublic function free( $public_key ) {\n\t\tif ( PHP_VERSION_ID < 80000 ) {\n\t\t\topenssl_free_key( $public_key ); // phpcs:ignore PHPCompatibility.FunctionUse.RemovedFunctions.openssl_free_keyDeprecated\n\t\t}\n\t}\n}\n";
    for (name, src, count, want) in [("export", export, 1, "issues"), ("notes", notes, 2, "issues"), ("jetpack", jetpack, 4, "compatible")] {
        let fx = Fixture::new("r4f6").file("plugin.php", src);
        let Some(mut fs) = real_phpcs(fx.path()) else {
            eprintln!("skipped: phpcs not installed in tools/php");
            return;
        };
        assert_eq!(fs.len(), count, "{name}: {:?}", sev(&fs));
        full(fx.path(), &mut fs);
        assert_eq!(verdict_of(&fs), want, "{name}: {:?}", sev(&fs));
    }
}

#[test]
fn r4_intl_successor_constant_ternary_inside_intl_check_is_guarded() {
    // jetpack class.json-api-endpoints.php (hidden by `phpcs:ignore` until annotations were ignored).
    let src = "<?php\nfunction amp_host( $host ) {\n\t\tif ( function_exists( 'idn_to_utf8' ) ) {\n\t\t\t// The third parameter is set explicitly to prevent issues with newer PHP versions compiled with an old ICU version.\n\t\t\t$variant = defined( 'INTL_IDNA_VARIANT_UTS46' )\n\t\t\t\t? INTL_IDNA_VARIANT_UTS46\n\t\t\t\t// phpcs:ignore PHPCompatibility.Constants.RemovedConstants.intl_idna_variant_2003DeprecatedRemoved\n\t\t\t\t: INTL_IDNA_VARIANT_2003; // @phan-suppress-current-line PhanUndeclaredConstant\n\t\t\t$host = idn_to_utf8( $host, IDNA_DEFAULT, $variant );\n\t\t}\n\t\treturn $host;\n}\n";
    let rule = "Constants.RemovedConstants.intl_idna_variant_2003DeprecatedRemoved";
    let msg = "The constant \"INTL_IDNA_VARIANT_2003\" is deprecated since PHP 7.2 and removed since PHP 8.0";
    let fx = Fixture::new("r4intl").file("a.php", src);
    let mut fs = vec![pfm(rule, "error", "a.php", line_of(src, ": INTL_IDNA_VARIANT_2003"), msg)];
    full(fx.path(), &mut fs);
    assert_eq!(fs[0].severity, "info", "{}", fs[0].message);
    assert_eq!(verdict_of(&fs), "compatible");
    // Without the intl check (the else side runs where intl is missing), on the then side, or
    // after the intl block closed, it stays an error.
    for s in [
        src.replace("if ( function_exists( 'idn_to_utf8' ) ) {", "if ( $x ) {"),
        src.replace("if ( function_exists( 'idn_to_utf8' ) ) {", "if ( ! function_exists( 'idn_to_utf8' ) ) {"),
        src.replace("? INTL_IDNA_VARIANT_UTS46\n\t\t\t\t// phpcs:ignore PHPCompatibility.Constants.RemovedConstants.intl_idna_variant_2003DeprecatedRemoved\n\t\t\t\t: INTL_IDNA_VARIANT_2003;", "? INTL_IDNA_VARIANT_2003\n\t\t\t\t// x\n\t\t\t\t: INTL_IDNA_VARIANT_UTS46;"),
        src.replace("if ( function_exists( 'idn_to_utf8' ) ) {", "if ( function_exists( 'idn_to_utf8' ) ) { $a = 1; }\n\t\t{"),
    ] {
        let fx = Fixture::new("r4intln").file("a.php", &s);
        let mut fs = vec![pfm(rule, "error", "a.php", line_of(&s, "INTL_IDNA_VARIANT_2003"), msg)];
        full(fx.path(), &mut fs);
        assert_eq!(fs[0].severity, "error", "{s}");
    }
}
