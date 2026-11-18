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
    assert_eq!(s("RemovedPredefinedGlobalVariables"), "warning");
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
