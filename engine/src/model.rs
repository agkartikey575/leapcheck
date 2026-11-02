//! Language-neutral data model shared by every analyzer.

use serde::{Deserialize, Serialize};

/// A problem that will (error) or may (warning) appear on the target runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub rule_id: String,
    /// "error" = breaks on target, "warning" = deprecated / risky.
    pub severity: String,
    /// "high" | "medium" | "low"
    pub confidence: String,
    pub message: String,
    pub file: String,
    pub line: u32,
    /// Name of the hidden/bundled component this file belongs to, if any.
    pub component: Option<String>,
    /// Which analyzer produced it, e.g. "php:phpcompatibility".
    pub analyzer: String,
}

/// A library found *inside* the scanned code (bundled, vendored or minified copy).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub version: Option<String>,
    /// Ecosystem of the component itself: "php", "npm", "js".
    pub kind: String,
    /// Path relative to the scan root.
    pub path: String,
    /// How we found it: "composer-installed", "package-json", "signature", "retirejs".
    pub detection: String,
    /// Known vulnerabilities for this version (from open data), if checked.
    pub known_vulnerabilities: u32,
}

/// A server extension / runtime module the code appears to need.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub name: String,
    pub evidence_file: String,
    pub evidence_line: u32,
}

/// Every analyzer implements this one contract.
pub trait Analyzer {
    /// Stable identifier, e.g. "php:phpcompatibility".
    fn id(&self) -> &'static str;
    /// Analyzer version, stored on every run for reproducibility.
    fn version(&self) -> String;
    /// Analyze `root` for compatibility with `target` (e.g. "8.4").
    fn analyze(&self, root: &std::path::Path, target: &str) -> anyhow::Result<Vec<Finding>>;
}
