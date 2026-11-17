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
    assert!(fs.iter().any(|f| f.rule_id == CARBON_RULE && f.file == "includes/class-my-date.php" && f.severity == "error"));
    assert!(fs.iter().any(|f| f.rule_id == CREATE_FUNCTION && f.severity == "error"));
}

#[test]
fn suppression_warning_action_downgrades_error_and_adds_reason() {
    let mut fs = vec![finding(ARGS_CHANGED, "error", "inc/util.php", 9), finding(ARGS_CHANGED, "warning", "inc/util.php", 20)];
    detect::apply_suppressions(&rules_dir(), &mut fs);
    assert_eq!(fs.len(), 2, "warning action must not remove findings");
    assert_eq!(fs[0].severity, "warning");
    assert!(fs[0].message.starts_with("test message ("), "{}", fs[0].message);
    assert!(fs[0].message.contains("func_get_args"), "{}", fs[0].message);
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
    crate::store(&conn, &run.id, "wordpress-plugin", "fixture", "1.0", &[], &[], per_target, encoded).unwrap();
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
        "vendor/nesbot/carbon/src/Carbon/Carbon.php:10 Changed warning/high nesbot/carbon@2.72.3",
    ];
    assert_eq!(got, expected, "\nactual:\n{}\n", got.join("\n"));
    let v = verdicts(&[("8.4".to_string(), fs)], 0);
    assert_eq!(v, vec![("php:8.4".to_string(), "breaks".to_string(), 1, 2)]);
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
