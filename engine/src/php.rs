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
                // Encoded/minified files and very large generated files are skipped.
                // Test suites of bundled libraries and node_modules never run on a live site.
                "--ignore=*.min.php,*/vendor/*/test/*,*/vendor/*/tests/*,*/vendor-prefixed/*/tests/*,*/vendor_prefixed/*/tests/*,*/node_modules/*",
            ])
            .arg(root)
            .output()
            .context("running phpcs")?;
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
        let mut findings = vec![];
        if let Some(files) = report.get("files").and_then(|f| f.as_object()) {
            for (file, v) in files {
                let rel = Path::new(file).strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| file.clone());
                for m in v.get("messages").and_then(|m| m.as_array()).into_iter().flatten() {
                    let sev = m.get("type").and_then(|t| t.as_str()).unwrap_or("WARNING").to_lowercase();
                    findings.push(Finding {
                        rule_id: m.get("source").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                        confidence: if sev == "error" { "high".into() } else { "medium".into() },
                        severity: sev,
                        message: m.get("message").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                        file: rel.clone(),
                        line: m.get("line").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
                        component: None,
                        analyzer: self.id().into(),
                    });
                }
            }
        }
        Ok(findings)
    }
}
