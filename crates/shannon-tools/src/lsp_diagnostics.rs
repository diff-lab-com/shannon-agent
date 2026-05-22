//! LSP Diagnostic Registry
//!
//! Provides a thread-safe registry for collecting, querying, and summarizing
//! LSP diagnostics (errors, warnings, hints, info) across files in a project.
//!
//! The registry maps file paths to their diagnostic lists and supports filtering
//! by severity, source, and computing aggregate summaries.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Diagnostic types
// ---------------------------------------------------------------------------

/// Severity of a diagnostic, matching LSP `DiagnosticSeverity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

impl DiagnosticSeverity {
    /// Convert to the LSP numeric severity value.
    pub fn to_lsp_number(self) -> u32 {
        match self {
            DiagnosticSeverity::Error => 1,
            DiagnosticSeverity::Warning => 2,
            DiagnosticSeverity::Info => 3,
            DiagnosticSeverity::Hint => 4,
        }
    }

    /// Create from an LSP numeric severity value.
    /// Falls back to `Info` for unknown values.
    pub fn from_lsp_number(n: u32) -> Self {
        match n {
            1 => DiagnosticSeverity::Error,
            2 => DiagnosticSeverity::Warning,
            3 => DiagnosticSeverity::Info,
            4 => DiagnosticSeverity::Hint,
            _ => DiagnosticSeverity::Info,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
            DiagnosticSeverity::Info => "info",
            DiagnosticSeverity::Hint => "hint",
        }
    }
}

impl std::fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Related information attached to a diagnostic, pointing to an additional
/// location that is related to the primary diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelatedInfo {
    /// File path of the related location.
    pub file_path: String,
    /// Description of the related information.
    pub message: String,
    /// (line, column) of the related location.
    pub location: (usize, usize),
}

/// A single diagnostic emitted by a language server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspDiagnostic {
    /// File path this diagnostic belongs to.
    pub file_path: String,
    /// 0-based line number.
    pub line: usize,
    /// 0-based column number.
    pub column: usize,
    /// Severity level.
    pub severity: DiagnosticSeverity,
    /// Diagnostic message.
    pub message: String,
    /// Source that produced this diagnostic (e.g., "rustc", "typescript").
    pub source: String,
    /// Optional diagnostic code (e.g., "E0382").
    pub code: Option<String>,
    /// Additional related information.
    pub related: Vec<RelatedInfo>,
}

impl LspDiagnostic {
    /// Convenience constructor.
    pub fn new(
        file_path: impl Into<String>,
        line: usize,
        column: usize,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            file_path: file_path.into(),
            line,
            column,
            severity,
            message: message.into(),
            source: source.into(),
            code: None,
            related: Vec::new(),
        }
    }

    /// Builder-style setter for `code`.
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Builder-style setter for `related`.
    pub fn with_related(mut self, related: Vec<RelatedInfo>) -> Self {
        self.related = related;
        self
    }

    /// Returns `true` if this diagnostic is an error.
    pub fn is_error(&self) -> bool {
        self.severity == DiagnosticSeverity::Error
    }

    /// Returns `true` if this diagnostic is a warning.
    pub fn is_warning(&self) -> bool {
        self.severity == DiagnosticSeverity::Warning
    }
}

// ---------------------------------------------------------------------------
// DiagnosticSummary
// ---------------------------------------------------------------------------

/// Aggregate summary of all diagnostics currently stored in the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub struct DiagnosticSummary {
    /// Total number of diagnostics.
    pub total: usize,
    /// Number of error-severity diagnostics.
    pub errors: usize,
    /// Number of warning-severity diagnostics.
    pub warnings: usize,
    /// Number of info-severity diagnostics.
    pub info: usize,
    /// Number of hint-severity diagnostics.
    pub hints: usize,
    /// List of files that contain at least one error.
    pub files_with_errors: Vec<String>,
    /// List of files that contain at least one diagnostic of any severity.
    pub files_with_diagnostics: Vec<String>,
}


impl std::fmt::Display for DiagnosticSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} total ({} errors, {} warnings, {} info, {} hints) across {} file(s)",
            self.total,
            self.errors,
            self.warnings,
            self.info,
            self.hints,
            self.files_with_diagnostics.len(),
        )
    }
}

// ---------------------------------------------------------------------------
// DiagnosticRegistry
// ---------------------------------------------------------------------------

/// Thread-safe registry that stores LSP diagnostics indexed by file path.
///
/// Supports updating diagnostics for individual files, querying by severity or
/// source, and computing aggregate summaries.
///
/// # Thread Safety
///
/// `DiagnosticRegistry` is `Send + Sync` and can be shared across threads
/// (e.g., wrapped in an `Arc`).
pub struct DiagnosticRegistry {
    diagnostics: HashMap<String, Vec<LspDiagnostic>>,
}

impl DiagnosticRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            diagnostics: HashMap::new(),
        }
    }

    /// Replace all diagnostics for a given file.
    ///
    /// If `diagnostics` is empty, the file entry is removed entirely.
    pub fn update(&mut self, file: &str, diagnostics: Vec<LspDiagnostic>) {
        if diagnostics.is_empty() {
            self.diagnostics.remove(file);
        } else {
            self.diagnostics.insert(file.to_string(), diagnostics);
        }
    }

    /// Get all diagnostics for a specific file. Returns an empty slice if the
    /// file has no diagnostics.
    pub fn get(&self, file: &str) -> &[LspDiagnostic] {
        self.diagnostics
            .get(file)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all diagnostics across all files.
    pub fn get_all(&self) -> Vec<&LspDiagnostic> {
        self.diagnostics.values().flatten().collect()
    }

    /// Get only error-severity diagnostics across all files.
    pub fn get_errors(&self) -> Vec<&LspDiagnostic> {
        self.diagnostics
            .values()
            .flatten()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect()
    }

    /// Get only warning-severity diagnostics across all files.
    pub fn get_warnings(&self) -> Vec<&LspDiagnostic> {
        self.diagnostics
            .values()
            .flatten()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .collect()
    }

    /// Get diagnostics filtered by their `source` field (e.g., "rustc").
    pub fn get_by_source(&self, source: &str) -> Vec<&LspDiagnostic> {
        self.diagnostics
            .values()
            .flatten()
            .filter(|d| d.source == source)
            .collect()
    }

    /// Clear all diagnostics for a single file.
    pub fn clear(&mut self, file: &str) {
        self.diagnostics.remove(file);
    }

    /// Clear all diagnostics for all files.
    pub fn clear_all(&mut self) {
        self.diagnostics.clear();
    }

    /// Compute an aggregate summary of all stored diagnostics.
    pub fn summary(&self) -> DiagnosticSummary {
        let mut total = 0usize;
        let mut errors = 0usize;
        let mut warnings = 0usize;
        let mut info = 0usize;
        let mut hints = 0usize;
        let mut files_with_errors = HashSet::new();
        let mut files_with_diagnostics = HashSet::new();

        for (file, diags) in &self.diagnostics {
            if !diags.is_empty() {
                files_with_diagnostics.insert(file.clone());
            }
            for d in diags {
                total += 1;
                match d.severity {
                    DiagnosticSeverity::Error => {
                        errors += 1;
                        files_with_errors.insert(file.clone());
                    }
                    DiagnosticSeverity::Warning => warnings += 1,
                    DiagnosticSeverity::Info => info += 1,
                    DiagnosticSeverity::Hint => hints += 1,
                }
            }
        }

        // Deterministic ordering
        let mut files_with_errors: Vec<String> = files_with_errors.into_iter().collect();
        let mut files_with_diagnostics: Vec<String> = files_with_diagnostics.into_iter().collect();
        files_with_errors.sort();
        files_with_diagnostics.sort();

        DiagnosticSummary {
            total,
            errors,
            warnings,
            info,
            hints,
            files_with_errors,
            files_with_diagnostics,
        }
    }

    /// Returns `true` if there is at least one error diagnostic across all
    /// files.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.values().flatten().any(|d| d.is_error())
    }

    /// Returns the number of files that have diagnostics.
    pub fn file_count(&self) -> usize {
        self.diagnostics.len()
    }

    /// Returns `true` if the registry contains no diagnostics at all.
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

impl Default for DiagnosticRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// CLI-based diagnostic runner
// ---------------------------------------------------------------------------

/// Result of running CLI diagnostics on a project.
pub struct CliDiagnosticResult {
    /// Parsed diagnostics.
    pub diagnostics: Vec<LspDiagnostic>,
    /// Whether the run succeeded (even if there are diagnostics).
    pub success: bool,
    /// Error message if the run itself failed.
    pub error: Option<String>,
}

/// Run CLI-based diagnostics for a project directory.
///
/// Detects the project type and runs the appropriate checker:
/// - Rust (`Cargo.toml` present): `cargo check --message-format=json`
///
/// Returns parsed diagnostics or an error if the checker couldn't run.
pub async fn run_cli_diagnostics(project_dir: &Path) -> CliDiagnosticResult {
    let has_cargo = project_dir.join("Cargo.toml").exists();

    if has_cargo {
        run_cargo_check(project_dir).await
    } else {
        CliDiagnosticResult {
            diagnostics: Vec::new(),
            success: true,
            error: None,
        }
    }
}

/// Run `cargo check --message-format=json` and parse diagnostics.
async fn run_cargo_check(project_dir: &Path) -> CliDiagnosticResult {
    let output = match tokio::process::Command::new("cargo")
        .args(["check", "--message-format=json", "--color=never"])
        .current_dir(project_dir)
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            return CliDiagnosticResult {
                diagnostics: Vec::new(),
                success: false,
                error: Some(format!("Failed to run cargo check: {e}")),
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut diagnostics = Vec::new();

    for line in stdout.lines() {
        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
            if msg.get("reason").and_then(|r| r.as_str()) == Some("compiler-message") {
                if let Some(diags) = parse_cargo_compiler_message(&msg) {
                    diagnostics.extend(diags);
                }
            }
        }
    }

    CliDiagnosticResult {
        diagnostics,
        success: true,
        error: None,
    }
}

/// Parse a single `compiler-message` JSON object from `cargo check` output.
fn parse_cargo_compiler_message(msg: &serde_json::Value) -> Option<Vec<LspDiagnostic>> {
    let message = msg.get("message")?;
    let level = message.get("level").and_then(|l| l.as_str()).unwrap_or("");
    let severity = match level {
        "error" => DiagnosticSeverity::Error,
        "warning" => DiagnosticSeverity::Warning,
        "note" | "failure-note" => DiagnosticSeverity::Info,
        _ => return None,
    };

    let code = message
        .get("code")
        .and_then(|c| c.get("code"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());

    let rendered = message
        .get("rendered")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();

    let msg_text = if rendered.is_empty() {
        message
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error")
            .to_string()
    } else {
        // Use first line of rendered as the message
        rendered.lines().next().unwrap_or("unknown error").to_string()
    };

    let mut diagnostics = Vec::new();

    // Parse spans for file/line info
    if let Some(spans) = message.get("spans").and_then(|s| s.as_array()) {
        for span in spans {
            let file = span
                .get("file_name")
                .and_then(|f| f.as_str())
                .unwrap_or("unknown");
            let line = span
                .get("line_start")
                .and_then(|l| l.as_u64())
                .unwrap_or(0) as usize;
            let column = span
                .get("column_start")
                .and_then(|c| c.as_u64())
                .unwrap_or(0) as usize;

            let mut diag = LspDiagnostic::new(file, line, column, severity, &msg_text, "rustc");
            if let Some(code) = &code {
                diag = diag.with_code(code);
            }
            diagnostics.push(diag);
        }
    }

    // If no spans, create a single diagnostic with unknown location
    if diagnostics.is_empty() {
        let mut diag = LspDiagnostic::new("unknown", 0, 0, severity, &msg_text, "rustc");
        if let Some(code) = &code {
            diag = diag.with_code(code);
        }
        diagnostics.push(diag);
    }

    Some(diagnostics)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Helper factories --

    fn make_error(file: &str, line: usize) -> LspDiagnostic {
        LspDiagnostic::new(file, line, 0, DiagnosticSeverity::Error, "test error", "rustc")
            .with_code("E0382")
    }

    fn make_warning(file: &str, line: usize) -> LspDiagnostic {
        LspDiagnostic::new(file, line, 0, DiagnosticSeverity::Warning, "unused var", "rustc")
    }

    fn make_info(file: &str, line: usize) -> LspDiagnostic {
        LspDiagnostic::new(file, line, 0, DiagnosticSeverity::Info, "info msg", "clippy")
    }

    fn make_hint(file: &str, line: usize) -> LspDiagnostic {
        LspDiagnostic::new(file, line, 0, DiagnosticSeverity::Hint, "try this", "rustc")
    }

    fn make_ts_error(file: &str, line: usize) -> LspDiagnostic {
        LspDiagnostic::new(file, line, 0, DiagnosticSeverity::Error, "ts error", "typescript")
    }

    // -- DiagnosticSeverity tests --

    #[test]
    fn test_severity_to_lsp_number() {
        assert_eq!(DiagnosticSeverity::Error.to_lsp_number(), 1);
        assert_eq!(DiagnosticSeverity::Warning.to_lsp_number(), 2);
        assert_eq!(DiagnosticSeverity::Info.to_lsp_number(), 3);
        assert_eq!(DiagnosticSeverity::Hint.to_lsp_number(), 4);
    }

    #[test]
    fn test_severity_from_lsp_number() {
        assert_eq!(DiagnosticSeverity::from_lsp_number(1), DiagnosticSeverity::Error);
        assert_eq!(DiagnosticSeverity::from_lsp_number(2), DiagnosticSeverity::Warning);
        assert_eq!(DiagnosticSeverity::from_lsp_number(3), DiagnosticSeverity::Info);
        assert_eq!(DiagnosticSeverity::from_lsp_number(4), DiagnosticSeverity::Hint);
        assert_eq!(DiagnosticSeverity::from_lsp_number(99), DiagnosticSeverity::Info);
    }

    #[test]
    fn test_severity_label() {
        assert_eq!(DiagnosticSeverity::Error.label(), "error");
        assert_eq!(DiagnosticSeverity::Warning.label(), "warning");
        assert_eq!(DiagnosticSeverity::Info.label(), "info");
        assert_eq!(DiagnosticSeverity::Hint.label(), "hint");
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", DiagnosticSeverity::Error), "error");
        assert_eq!(format!("{}", DiagnosticSeverity::Hint), "hint");
    }

    #[test]
    fn test_severity_roundtrip() {
        for s in [
            DiagnosticSeverity::Error,
            DiagnosticSeverity::Warning,
            DiagnosticSeverity::Info,
            DiagnosticSeverity::Hint,
        ] {
            assert_eq!(DiagnosticSeverity::from_lsp_number(s.to_lsp_number()), s);
        }
    }

    // -- LspDiagnostic tests --

    #[test]
    fn test_diagnostic_new() {
        let d = LspDiagnostic::new("foo.rs", 10, 5, DiagnosticSeverity::Error, "msg", "rustc");
        assert_eq!(d.file_path, "foo.rs");
        assert_eq!(d.line, 10);
        assert_eq!(d.column, 5);
        assert_eq!(d.severity, DiagnosticSeverity::Error);
        assert_eq!(d.message, "msg");
        assert_eq!(d.source, "rustc");
        assert!(d.code.is_none());
        assert!(d.related.is_empty());
    }

    #[test]
    fn test_diagnostic_builder_code() {
        let d = LspDiagnostic::new("f.rs", 0, 0, DiagnosticSeverity::Error, "m", "s")
            .with_code("E0001");
        assert_eq!(d.code.as_deref(), Some("E0001"));
    }

    #[test]
    fn test_diagnostic_builder_related() {
        let related = vec![RelatedInfo {
            file_path: "other.rs".to_string(),
            message: "related msg".to_string(),
            location: (5, 10),
        }];
        let d = LspDiagnostic::new("f.rs", 0, 0, DiagnosticSeverity::Error, "m", "s")
            .with_related(related);
        assert_eq!(d.related.len(), 1);
        assert_eq!(d.related[0].file_path, "other.rs");
    }

    #[test]
    fn test_diagnostic_is_error() {
        let err = LspDiagnostic::new("f.rs", 0, 0, DiagnosticSeverity::Error, "", "s");
        let warn = LspDiagnostic::new("f.rs", 0, 0, DiagnosticSeverity::Warning, "", "s");
        assert!(err.is_error());
        assert!(!warn.is_error());
    }

    #[test]
    fn test_diagnostic_is_warning() {
        let warn = LspDiagnostic::new("f.rs", 0, 0, DiagnosticSeverity::Warning, "", "s");
        let err = LspDiagnostic::new("f.rs", 0, 0, DiagnosticSeverity::Error, "", "s");
        assert!(warn.is_warning());
        assert!(!err.is_warning());
    }

    #[test]
    fn test_diagnostic_serialization() {
        let d = LspDiagnostic::new("test.rs", 10, 5, DiagnosticSeverity::Error, "oops", "rustc")
            .with_code("E0382");
        let json_str = serde_json::to_string(&d).unwrap();
        assert!(json_str.contains("\"severity\":\"Error\""));
        assert!(json_str.contains("\"code\":\"E0382\""));
        assert!(json_str.contains("\"file_path\":\"test.rs\""));
    }

    #[test]
    fn test_diagnostic_deserialization() {
        let json_str = r#"{
            "file_path": "test.rs",
            "line": 10,
            "column": 5,
            "severity": "Error",
            "message": "oops",
            "source": "rustc",
            "code": "E0382",
            "related": []
        }"#;
        let d: LspDiagnostic = serde_json::from_str(json_str).unwrap();
        assert_eq!(d.file_path, "test.rs");
        assert_eq!(d.line, 10);
        assert_eq!(d.severity, DiagnosticSeverity::Error);
        assert_eq!(d.code.as_deref(), Some("E0382"));
    }

    #[test]
    fn test_diagnostic_equality() {
        let a = LspDiagnostic::new("f.rs", 1, 0, DiagnosticSeverity::Error, "msg", "src");
        let b = LspDiagnostic::new("f.rs", 1, 0, DiagnosticSeverity::Error, "msg", "src");
        let c = LspDiagnostic::new("f.rs", 2, 0, DiagnosticSeverity::Error, "msg", "src");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // -- DiagnosticRegistry: new / default --

    #[test]
    fn test_registry_new_is_empty() {
        let reg = DiagnosticRegistry::new();
        assert!(reg.is_empty());
        assert!(!reg.has_errors());
        assert_eq!(reg.file_count(), 0);
    }

    #[test]
    fn test_registry_default() {
        let reg = DiagnosticRegistry::default();
        assert!(reg.is_empty());
    }

    // -- DiagnosticRegistry: update / get --

    #[test]
    fn test_registry_update_and_get() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1), make_warning("a.rs", 2)]);
        assert_eq!(reg.get("a.rs").len(), 2);
        assert_eq!(reg.get("a.rs")[0].line, 1);
        assert_eq!(reg.get("a.rs")[1].line, 2);
    }

    #[test]
    fn test_registry_get_missing_file() {
        let reg = DiagnosticRegistry::new();
        assert!(reg.get("nonexistent.rs").is_empty());
    }

    #[test]
    fn test_registry_update_replaces() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1)]);
        reg.update("a.rs", vec![make_warning("a.rs", 5)]);
        let diags = reg.get("a.rs");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_registry_update_empty_removes_entry() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1)]);
        assert!(!reg.is_empty());
        reg.update("a.rs", vec![]);
        assert!(reg.is_empty());
        assert!(reg.get("a.rs").is_empty());
    }

    // -- DiagnosticRegistry: get_all / get_errors / get_warnings / get_by_source --

    #[test]
    fn test_registry_get_all() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1), make_warning("a.rs", 2)]);
        reg.update("b.rs", vec![make_info("b.rs", 3)]);
        assert_eq!(reg.get_all().len(), 3);
    }

    #[test]
    fn test_registry_get_errors() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1), make_warning("a.rs", 2)]);
        reg.update("b.rs", vec![make_error("b.rs", 3), make_hint("b.rs", 4)]);
        let errors = reg.get_errors();
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|d| d.is_error()));
    }

    #[test]
    fn test_registry_get_warnings() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1), make_warning("a.rs", 2)]);
        let warnings = reg.get_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].is_warning());
    }

    #[test]
    fn test_registry_get_by_source() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1), make_warning("a.rs", 2)]);
        reg.update("b.rs", vec![make_ts_error("b.rs", 3)]);
        let rustc = reg.get_by_source("rustc");
        assert_eq!(rustc.len(), 2);
        let ts = reg.get_by_source("typescript");
        assert_eq!(ts.len(), 1);
        let none = reg.get_by_source("unknown");
        assert!(none.is_empty());
    }

    // -- DiagnosticRegistry: clear / clear_all --

    #[test]
    fn test_registry_clear_file() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1)]);
        reg.update("b.rs", vec![make_warning("b.rs", 2)]);
        reg.clear("a.rs");
        assert!(reg.get("a.rs").is_empty());
        assert_eq!(reg.get("b.rs").len(), 1);
    }

    #[test]
    fn test_registry_clear_all() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1)]);
        reg.update("b.rs", vec![make_warning("b.rs", 2)]);
        reg.clear_all();
        assert!(reg.is_empty());
        assert!(reg.get("a.rs").is_empty());
        assert!(reg.get("b.rs").is_empty());
    }

    // -- DiagnosticRegistry: summary --

    #[test]
    fn test_registry_summary_empty() {
        let reg = DiagnosticRegistry::new();
        let s = reg.summary();
        assert_eq!(s, DiagnosticSummary::default());
    }

    #[test]
    fn test_registry_summary_mixed() {
        let mut reg = DiagnosticRegistry::new();
        reg.update(
            "a.rs",
            vec![
                make_error("a.rs", 1),
                make_error("a.rs", 2),
                make_warning("a.rs", 3),
            ],
        );
        reg.update(
            "b.rs",
            vec![make_info("b.rs", 4), make_hint("b.rs", 5)],
        );
        reg.update(
            "c.rs",
            vec![make_warning("c.rs", 6)],
        );

        let s = reg.summary();
        assert_eq!(s.total, 6);
        assert_eq!(s.errors, 2);
        assert_eq!(s.warnings, 2);
        assert_eq!(s.info, 1);
        assert_eq!(s.hints, 1);
        assert_eq!(s.files_with_errors, vec!["a.rs"]);
        // files_with_diagnostics is sorted
        assert_eq!(s.files_with_diagnostics, vec!["a.rs", "b.rs", "c.rs"]);
    }

    #[test]
    fn test_registry_summary_display() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1)]);
        let s = reg.summary();
        let text = format!("{s}");
        assert!(text.contains("1 total"));
        assert!(text.contains("1 errors"));
        assert!(text.contains("0 warnings"));
        assert!(text.contains("1 file(s)"));
    }

    // -- DiagnosticRegistry: has_errors --

    #[test]
    fn test_registry_has_errors_true() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1)]);
        assert!(reg.has_errors());
    }

    #[test]
    fn test_registry_has_errors_false() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_warning("a.rs", 1)]);
        assert!(!reg.has_errors());
    }

    #[test]
    fn test_registry_has_errors_empty() {
        let reg = DiagnosticRegistry::new();
        assert!(!reg.has_errors());
    }

    // -- DiagnosticRegistry: file_count / is_empty --

    #[test]
    fn test_registry_file_count() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1)]);
        reg.update("b.rs", vec![make_warning("b.rs", 2)]);
        assert_eq!(reg.file_count(), 2);
    }

    #[test]
    fn test_registry_file_count_after_clear() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1)]);
        reg.clear("a.rs");
        assert_eq!(reg.file_count(), 0);
    }

    // -- Send + Sync --

    #[test]
    fn test_registry_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DiagnosticRegistry>();
        assert_send_sync::<LspDiagnostic>();
        assert_send_sync::<DiagnosticSummary>();
        assert_send_sync::<DiagnosticSeverity>();
        assert_send_sync::<RelatedInfo>();
    }

    // -- RelatedInfo serialization --

    #[test]
    fn test_related_info_serialization() {
        let info = RelatedInfo {
            file_path: "other.rs".to_string(),
            message: "see here".to_string(),
            location: (10, 5),
        };
        let json_str = serde_json::to_string(&info).unwrap();
        assert!(json_str.contains("\"file_path\":\"other.rs\""));
        assert!(json_str.contains("\"message\":\"see here\""));
        assert!(json_str.contains("\"location\":[10,5]"));
    }

    #[test]
    fn test_related_info_deserialization() {
        let json_str = r#"{"file_path":"other.rs","message":"see here","location":[10,5]}"#;
        let info: RelatedInfo = serde_json::from_str(json_str).unwrap();
        assert_eq!(info.file_path, "other.rs");
        assert_eq!(info.message, "see here");
        assert_eq!(info.location, (10, 5));
    }

    // -- DiagnosticSummary serialization --

    #[test]
    fn test_summary_serialization() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1)]);
        let s = reg.summary();
        let json_str = serde_json::to_string(&s).unwrap();
        assert!(json_str.contains("\"total\":1"));
        assert!(json_str.contains("\"errors\":1"));
        assert!(json_str.contains("\"files_with_errors\":[\"a.rs\"]"));
    }

    // -- Multi-file scenarios --

    #[test]
    fn test_registry_multiple_updates_same_file() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1), make_warning("a.rs", 2)]);
        reg.update("a.rs", vec![make_error("a.rs", 10), make_hint("a.rs", 20), make_info("a.rs", 30)]);
        assert_eq!(reg.get("a.rs").len(), 3);
        assert_eq!(reg.get("a.rs")[0].line, 10);
        assert_eq!(reg.get("a.rs")[1].line, 20);
        assert_eq!(reg.get("a.rs")[2].line, 30);
    }

    #[test]
    fn test_registry_clear_nonexistent_file_does_not_panic() {
        let mut reg = DiagnosticRegistry::new();
        reg.clear("nonexistent.rs"); // should not panic
    }

    #[test]
    fn test_registry_get_by_source_empty_registry() {
        let reg = DiagnosticRegistry::new();
        assert!(reg.get_by_source("rustc").is_empty());
    }

    #[test]
    fn test_registry_get_all_empty() {
        let reg = DiagnosticRegistry::new();
        assert!(reg.get_all().is_empty());
    }

    #[test]
    fn test_registry_get_errors_none() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_warning("a.rs", 1), make_hint("a.rs", 2)]);
        assert!(reg.get_errors().is_empty());
    }

    #[test]
    fn test_registry_get_warnings_none() {
        let mut reg = DiagnosticRegistry::new();
        reg.update("a.rs", vec![make_error("a.rs", 1)]);
        assert!(reg.get_warnings().is_empty());
    }

    // -- CLI diagnostic runner tests --

    #[test]
    fn test_parse_cargo_compiler_message_error() {
        let msg = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "level": "error",
                "message": "cannot find value `x` in this scope",
                "code": { "code": "E0425" },
                "spans": [
                    {
                        "file_name": "src/main.rs",
                        "line_start": 10,
                        "column_start": 5
                    }
                ],
                "rendered": "error[E0425]: cannot find value `x` in this scope\n --> src/main.rs:10:5\n"
            }
        });
        let diags = parse_cargo_compiler_message(&msg).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].file_path, "src/main.rs");
        assert_eq!(diags[0].line, 10);
        assert_eq!(diags[0].column, 5);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diags[0].source, "rustc");
        assert_eq!(diags[0].code.as_deref(), Some("E0425"));
    }

    #[test]
    fn test_parse_cargo_compiler_message_warning() {
        let msg = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "level": "warning",
                "message": "unused variable: `y`",
                "spans": [
                    {
                        "file_name": "src/lib.rs",
                        "line_start": 5,
                        "column_start": 9
                    }
                ],
                "rendered": ""
            }
        });
        let diags = parse_cargo_compiler_message(&msg).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
        assert_eq!(diags[0].file_path, "src/lib.rs");
    }

    #[test]
    fn test_parse_cargo_compiler_message_no_spans() {
        let msg = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "level": "error",
                "message": "build error",
                "spans": [],
                "rendered": "error: build error\n"
            }
        });
        let diags = parse_cargo_compiler_message(&msg).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].file_path, "unknown");
        assert_eq!(diags[0].line, 0);
    }

    #[test]
    fn test_parse_cargo_compiler_message_note_is_info() {
        let msg = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "level": "note",
                "message": "some note",
                "spans": [],
                "rendered": ""
            }
        });
        let diags = parse_cargo_compiler_message(&msg).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Info);
    }

    #[test]
    fn test_parse_cargo_compiler_message_missing_message() {
        let msg = serde_json::json!({ "reason": "compiler-message" });
        assert!(parse_cargo_compiler_message(&msg).is_none());
    }

    #[test]
    fn test_parse_cargo_compiler_message_multiple_spans() {
        let msg = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "level": "error",
                "message": "mismatched types",
                "spans": [
                    { "file_name": "src/a.rs", "line_start": 1, "column_start": 1 },
                    { "file_name": "src/b.rs", "line_start": 2, "column_start": 3 }
                ],
                "rendered": ""
            }
        });
        let diags = parse_cargo_compiler_message(&msg).unwrap();
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].file_path, "src/a.rs");
        assert_eq!(diags[1].file_path, "src/b.rs");
    }

    #[tokio::test]
    async fn test_run_cli_diagnostics_no_project() {
        let temp = tempfile::tempdir().unwrap();
        let result = run_cli_diagnostics(temp.path()).await;
        assert!(result.success);
        assert!(result.diagnostics.is_empty());
        assert!(result.error.is_none());
    }
}
