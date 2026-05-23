//! LSP display bridge for showing code intelligence in terminal
//!
//! Displays diagnostics, type info, and symbol references inline in chat.
//! Data is received from external sources (MCP tools, shannon-core), not from a running LSP server.

use crate::theme::Theme;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// A diagnostic item (error, warning, info)
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub file_path: String,
    pub line: usize,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// Type information for a symbol
#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub symbol: String,
    pub type_text: String,
    pub documentation: Option<String>,
    pub file_path: Option<String>,
    pub line: Option<usize>,
}

/// LSP display renderer
pub struct LspDisplay;

impl LspDisplay {
    /// Render diagnostics as styled lines
    pub fn render_diagnostics(diagnostics: &[Diagnostic], theme: &Theme) -> Vec<Line<'static>> {
        diagnostics
            .iter()
            .map(|diag| {
                let icon = Self::severity_icon(diag.severity);
                let color = Self::severity_color(diag.severity, theme);
                let source = diag.source.as_deref().unwrap_or("diag");

                let location = if diag.line > 0 {
                    format!("{}:{}", diag.file_path, diag.line)
                } else {
                    diag.file_path.clone()
                };

                Line::from(vec![
                    Span::styled(
                        format!("[{icon}] "),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(location, Style::default().fg(theme.text_dim)),
                    Span::raw(": "),
                    Span::styled(diag.message.clone(), Style::default().fg(color)),
                    Span::styled(
                        format!(" ({source})"),
                        Style::default()
                            .fg(theme.text_dim)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ])
            })
            .collect()
    }

    /// Render type info as a hover popup
    pub fn render_type_info(info: &TypeInfo, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // Clone data to avoid lifetime issues
        let symbol = info.symbol.clone();
        let type_text = info.type_text.clone();
        let documentation = info.documentation.clone();
        let file_path = info.file_path.clone();
        let line = info.line;

        // Header: symbol with type
        lines.push(Line::from(vec![
            Span::styled(
                symbol,
                Style::default()
                    .fg(theme.syntax_function)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": "),
            Span::styled(type_text, Style::default().fg(theme.syntax_type)),
        ]));

        // Location if available
        if let (Some(path), Some(ln)) = (file_path, line) {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{path}:{ln}"), Style::default().fg(theme.text_dim)),
            ]));
        }

        // Documentation if available
        if let Some(doc) = documentation {
            for line_str in textwrap::wrap(doc.as_str(), 80) {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        line_str.into_owned(),
                        Style::default()
                            .fg(theme.text_dim)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }
        }

        lines
    }

    /// Render inline diagnostics for code blocks
    /// Returns annotation lines to insert after code lines
    pub fn render_inline_diagnostic(
        message: &str,
        severity: DiagnosticSeverity,
        col: usize,
        theme: &Theme,
    ) -> Line<'static> {
        let color = Self::severity_color(severity, theme);
        let indent = " ".repeat(col.saturating_sub(1));

        Line::from(vec![
            Span::raw(indent),
            Span::styled("~~~", Style::default().fg(color)),
            Span::raw(" "),
            Span::styled(
                message.to_string(),
                Style::default().fg(color).add_modifier(Modifier::ITALIC),
            ),
        ])
    }

    /// Format a diagnostic severity icon
    fn severity_icon(severity: DiagnosticSeverity) -> &'static str {
        match severity {
            DiagnosticSeverity::Error => "E",
            DiagnosticSeverity::Warning => "W",
            DiagnosticSeverity::Info => "I",
            DiagnosticSeverity::Hint => "H",
        }
    }

    /// Get color for severity
    fn severity_color(severity: DiagnosticSeverity, theme: &Theme) -> Color {
        match severity {
            DiagnosticSeverity::Error => theme.error,
            DiagnosticSeverity::Warning => theme.warning,
            DiagnosticSeverity::Info => theme.primary,
            DiagnosticSeverity::Hint => theme.text_dim,
        }
    }
}

/// Manages diagnostic state for display
#[derive(Debug, Clone)]
pub struct DiagnosticStore {
    pub diagnostics: Vec<Diagnostic>,
    stale: bool,
}

impl DiagnosticStore {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
            stale: false,
        }
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    pub fn is_stale(&self) -> bool {
        self.stale
    }

    pub fn clear_stale(&mut self) {
        self.stale = false;
    }

    pub fn add(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }

    pub fn clear(&mut self) {
        self.diagnostics.clear();
    }

    pub fn get_for_file(&self, path: &str) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.file_path == path)
            .collect()
    }

    pub fn get_for_line(&self, path: &str, line: usize) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.file_path == path && d.line == line)
            .collect()
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| matches!(d.severity, DiagnosticSeverity::Error))
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| matches!(d.severity, DiagnosticSeverity::Error))
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| matches!(d.severity, DiagnosticSeverity::Warning))
            .count()
    }
}

impl Default for DiagnosticStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticStore {
    /// Sync diagnostics from the tools-layer DiagnosticRegistry into this display store.
    ///
    /// Clears existing diagnostics and replaces them with the registry's current data.
    /// Returns the number of diagnostics synced.
    pub fn sync_from_registry(&mut self, registry: &shannon_tools::DiagnosticRegistry) -> usize {
        self.diagnostics.clear();
        self.stale = false;
        for diag in registry.get_all() {
            let severity = match diag.severity {
                shannon_tools::DiagnosticSeverity::Error => DiagnosticSeverity::Error,
                shannon_tools::DiagnosticSeverity::Warning => DiagnosticSeverity::Warning,
                shannon_tools::DiagnosticSeverity::Info => DiagnosticSeverity::Info,
                shannon_tools::DiagnosticSeverity::Hint => DiagnosticSeverity::Hint,
            };
            self.diagnostics.push(Diagnostic {
                severity,
                message: diag.message.clone(),
                file_path: diag.file_path.clone(),
                line: diag.line + 1, // LSP diagnostics are 0-based, display is 1-based
                source: Some(diag.source.clone()),
            });
        }
        self.diagnostics.len()
    }

    /// Update diagnostics from a CLI diagnostic run result.
    ///
    /// Replaces all existing diagnostics with the parsed CLI output.
    /// Returns the number of diagnostics loaded.
    pub fn update_from_cli(&mut self, result: &shannon_tools::CliDiagnosticResult) -> usize {
        self.diagnostics.clear();
        self.stale = false;
        for diag in &result.diagnostics {
            let severity = match diag.severity {
                shannon_tools::DiagnosticSeverity::Error => DiagnosticSeverity::Error,
                shannon_tools::DiagnosticSeverity::Warning => DiagnosticSeverity::Warning,
                shannon_tools::DiagnosticSeverity::Info => DiagnosticSeverity::Info,
                shannon_tools::DiagnosticSeverity::Hint => DiagnosticSeverity::Hint,
            };
            self.diagnostics.push(Diagnostic {
                severity,
                message: diag.message.clone(),
                file_path: diag.file_path.clone(),
                line: diag.line + 1,
                source: Some(diag.source.clone()),
            });
        }
        self.diagnostics.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_store_empty() {
        let store = DiagnosticStore::new();
        assert!(!store.has_errors());
        assert_eq!(store.error_count(), 0);
        assert_eq!(store.warning_count(), 0);
    }

    #[test]
    fn test_diagnostic_store_add() {
        let mut store = DiagnosticStore::new();
        store.add(Diagnostic {
            severity: DiagnosticSeverity::Error,
            message: "test error".to_string(),
            file_path: "test.rs".to_string(),
            line: 10,
            source: Some("rustc".to_string()),
        });
        assert!(store.has_errors());
        assert_eq!(store.error_count(), 1);
    }

    #[test]
    fn test_diagnostic_store_clear() {
        let mut store = DiagnosticStore::new();
        store.add(Diagnostic {
            severity: DiagnosticSeverity::Error,
            message: "test".to_string(),
            file_path: "test.rs".to_string(),
            line: 1,
            source: None,
        });
        store.clear();
        assert!(!store.has_errors());
    }

    #[test]
    fn test_diagnostic_store_filter_by_file() {
        let mut store = DiagnosticStore::new();
        store.add(Diagnostic {
            severity: DiagnosticSeverity::Error,
            message: "error1".to_string(),
            file_path: "file1.rs".to_string(),
            line: 5,
            source: None,
        });
        store.add(Diagnostic {
            severity: DiagnosticSeverity::Warning,
            message: "warning1".to_string(),
            file_path: "file2.rs".to_string(),
            line: 10,
            source: None,
        });

        let file1_diags = store.get_for_file("file1.rs");
        assert_eq!(file1_diags.len(), 1);
        assert_eq!(file1_diags[0].message, "error1");
    }

    #[test]
    fn test_diagnostic_store_filter_by_line() {
        let mut store = DiagnosticStore::new();
        store.add(Diagnostic {
            severity: DiagnosticSeverity::Error,
            message: "line 5 error".to_string(),
            file_path: "test.rs".to_string(),
            line: 5,
            source: None,
        });
        store.add(Diagnostic {
            severity: DiagnosticSeverity::Warning,
            message: "line 10 warning".to_string(),
            file_path: "test.rs".to_string(),
            line: 10,
            source: None,
        });

        let line5_diags = store.get_for_line("test.rs", 5);
        assert_eq!(line5_diags.len(), 1);
        assert_eq!(line5_diags[0].message, "line 5 error");
    }

    #[test]
    fn test_render_diagnostics() {
        let theme = Theme::default_dark();
        let diagnostics = vec![Diagnostic {
            severity: DiagnosticSeverity::Error,
            message: "unexpected type".to_string(),
            file_path: "src/main.rs".to_string(),
            line: 42,
            source: Some("rustc".to_string()),
        }];

        let lines = LspDisplay::render_diagnostics(&diagnostics, &theme);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_render_type_info() {
        let theme = Theme::default_dark();
        let info = TypeInfo {
            symbol: "foo".to_string(),
            type_text: "fn() -> i32".to_string(),
            documentation: Some("Does foo things".to_string()),
            file_path: Some("src/lib.rs".to_string()),
            line: Some(10),
        };

        let lines = LspDisplay::render_type_info(&info, &theme);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_render_inline_diagnostic() {
        let theme = Theme::default_dark();
        let line = LspDisplay::render_inline_diagnostic(
            "missing semicolon",
            DiagnosticSeverity::Error,
            5,
            &theme,
        );
        // Should have indentation + squiggly + message
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("~~~"));
        assert!(text.contains("missing semicolon"));
    }

    #[test]
    fn test_severity_icon() {
        assert_eq!(LspDisplay::severity_icon(DiagnosticSeverity::Error), "E");
        assert_eq!(LspDisplay::severity_icon(DiagnosticSeverity::Warning), "W");
        assert_eq!(LspDisplay::severity_icon(DiagnosticSeverity::Info), "I");
        assert_eq!(LspDisplay::severity_icon(DiagnosticSeverity::Hint), "H");
    }

    #[test]
    fn test_sync_from_registry_empty() {
        let mut store = DiagnosticStore::new();
        let registry = shannon_tools::DiagnosticRegistry::new();
        let count = store.sync_from_registry(&registry);
        assert_eq!(count, 0);
        assert!(store.diagnostics.is_empty());
    }

    #[test]
    fn test_sync_from_registry_with_diagnostics() {
        let mut registry = shannon_tools::DiagnosticRegistry::new();
        registry.update(
            "src/main.rs",
            vec![
                shannon_tools::LspDiagnostic::new(
                    "src/main.rs",
                    9,
                    4,
                    shannon_tools::DiagnosticSeverity::Error,
                    "mismatched types",
                    "rustc",
                ),
                shannon_tools::LspDiagnostic::new(
                    "src/main.rs",
                    20,
                    0,
                    shannon_tools::DiagnosticSeverity::Warning,
                    "unused variable",
                    "rustc",
                ),
            ],
        );

        let mut store = DiagnosticStore::new();
        let count = store.sync_from_registry(&registry);
        assert_eq!(count, 2);
        assert_eq!(store.error_count(), 1);
        assert_eq!(store.warning_count(), 1);

        // Line numbers should be 1-based (LSP 0-based + 1)
        assert_eq!(store.diagnostics[0].line, 10);
        assert_eq!(store.diagnostics[0].source, Some("rustc".to_string()));
    }

    #[test]
    fn test_diagnostic_store_stale_flag() {
        let mut store = DiagnosticStore::new();
        assert!(!store.is_stale());
        store.mark_stale();
        assert!(store.is_stale());
        store.clear_stale();
        assert!(!store.is_stale());
    }

    #[test]
    fn test_sync_from_registry_replaces_existing() {
        let mut store = DiagnosticStore::new();
        store.add(Diagnostic {
            severity: DiagnosticSeverity::Error,
            message: "old".to_string(),
            file_path: "old.rs".to_string(),
            line: 1,
            source: None,
        });
        assert_eq!(store.diagnostics.len(), 1);

        let registry = shannon_tools::DiagnosticRegistry::new();
        let count = store.sync_from_registry(&registry);
        assert_eq!(count, 0);
        assert!(store.diagnostics.is_empty());
    }

    #[test]
    fn test_update_from_cli_with_diagnostics() {
        let mut store = DiagnosticStore::new();
        store.mark_stale();
        assert!(store.is_stale());

        let result = shannon_tools::CliDiagnosticResult {
            diagnostics: vec![
                shannon_tools::LspDiagnostic::new(
                    "src/main.rs",
                    9,
                    0,
                    shannon_tools::DiagnosticSeverity::Error,
                    "cannot find value `x`",
                    "rustc",
                ),
                shannon_tools::LspDiagnostic::new(
                    "src/lib.rs",
                    4,
                    0,
                    shannon_tools::DiagnosticSeverity::Warning,
                    "unused variable",
                    "rustc",
                ),
            ],
            success: true,
            error: None,
        };

        let count = store.update_from_cli(&result);
        assert_eq!(count, 2);
        assert!(!store.is_stale());
        assert_eq!(store.error_count(), 1);
        assert_eq!(store.warning_count(), 1);
        assert_eq!(store.diagnostics[0].line, 10); // 0-based + 1
    }

    #[test]
    fn test_update_from_cli_empty() {
        let mut store = DiagnosticStore::new();
        store.add(Diagnostic {
            severity: DiagnosticSeverity::Error,
            message: "old".to_string(),
            file_path: "old.rs".to_string(),
            line: 1,
            source: None,
        });
        assert_eq!(store.diagnostics.len(), 1);

        let result = shannon_tools::CliDiagnosticResult {
            diagnostics: vec![],
            success: true,
            error: None,
        };

        let count = store.update_from_cli(&result);
        assert_eq!(count, 0);
        assert!(store.diagnostics.is_empty());
    }

    #[test]
    fn test_update_from_cli_severity_mapping() {
        let mut store = DiagnosticStore::new();

        let result = shannon_tools::CliDiagnosticResult {
            diagnostics: vec![
                shannon_tools::LspDiagnostic::new(
                    "a.rs",
                    0,
                    0,
                    shannon_tools::DiagnosticSeverity::Error,
                    "e",
                    "rustc",
                ),
                shannon_tools::LspDiagnostic::new(
                    "b.rs",
                    0,
                    0,
                    shannon_tools::DiagnosticSeverity::Warning,
                    "w",
                    "rustc",
                ),
                shannon_tools::LspDiagnostic::new(
                    "c.rs",
                    0,
                    0,
                    shannon_tools::DiagnosticSeverity::Info,
                    "i",
                    "rustc",
                ),
                shannon_tools::LspDiagnostic::new(
                    "d.rs",
                    0,
                    0,
                    shannon_tools::DiagnosticSeverity::Hint,
                    "h",
                    "rustc",
                ),
            ],
            success: true,
            error: None,
        };

        let count = store.update_from_cli(&result);
        assert_eq!(count, 4);
        assert!(matches!(
            store.diagnostics[0].severity,
            DiagnosticSeverity::Error
        ));
        assert!(matches!(
            store.diagnostics[1].severity,
            DiagnosticSeverity::Warning
        ));
        assert!(matches!(
            store.diagnostics[2].severity,
            DiagnosticSeverity::Info
        ));
        assert!(matches!(
            store.diagnostics[3].severity,
            DiagnosticSeverity::Hint
        ));
    }
}
