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
                "--ignore=*.min.php",
            ])
            .arg(root)
            .output()
            .context("running phpcs")?;
        // phpcs exits 0 (clean), 1/2 (issues found). Anything else is a real failure.
        let code = out.status.code().unwrap_or(-1);
        if !(0..=2).contains(&code) {
            bail!("phpcs failed (exit {code}): {}", String::from_utf8_lossy(&out.stderr));
        }
        let report: serde_json::Value = serde_json::from_slice(&out.stdout)
            .with_context(|| format!("phpcs output was not JSON: {}", String::from_utf8_lossy(&out.stdout).chars().take(300).collect::<String>()))?;
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
