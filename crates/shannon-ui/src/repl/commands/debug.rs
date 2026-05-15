use crate::{widgets::ChatRole, Result};

use super::super::Repl;

pub(crate) fn handle_select_tools(repl: &mut Repl) -> Result<()> {
    let tool_info = if let Some(ref engine) = repl.query_engine {
        engine.tools().list_tools_info()
    } else {
        Vec::new()
    };

    let items: Vec<crate::widgets::select::SelectItem<String>> = tool_info.iter().map(|info| {
        crate::widgets::select::SelectItem::new(info.name.clone(), info.name.clone())
            .with_description(info.description.clone())
    }).collect();

    let widget = crate::widgets::select::MultiSelectWidget::new("Select Tools".to_string())
        .with_items(items);

    repl.state.multi_select = Some(widget);
    Ok(())
}

pub(crate) fn handle_debug(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::debug_utils::{
        parse_debug_subcommand, parse_log_level,
        format_debug_help, format_log_response,
        format_profile_response, format_trace_response,
        format_system_info, DebugSubcommand,
    };

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let subcommand_str = parts.first().copied().unwrap_or("");
    let subcommand = parse_debug_subcommand(subcommand_str);

    let output = match subcommand {
        DebugSubcommand::Help => format_debug_help(),
        DebugSubcommand::Info => {
            let mut info = format_system_info();
            if let Ok(rust_output) = std::process::Command::new("rustc").arg("--version").output() {
                let version = String::from_utf8_lossy(&rust_output.stdout);
                if !version.trim().is_empty() {
                    info.push_str(&format!("  Rust: {}\n", version.trim()));
                }
            }
            if let Ok(cargo_output) = std::process::Command::new("cargo").arg("--version").output() {
                let version = String::from_utf8_lossy(&cargo_output.stdout);
                if !version.trim().is_empty() {
                    info.push_str(&format!("  Cargo: {}\n", version.trim()));
                }
            }
            info
        }
        DebugSubcommand::Log => {
            let level_str = parts.get(1).copied().unwrap_or("info");
            let level = parse_log_level(level_str);
            if let Some(lvl) = level {
                // SAFETY: REPL event loop is single-threaded; no concurrent reads of RUST_LOG.
                unsafe { std::env::set_var("RUST_LOG", lvl.to_string()); }
            }
            format_log_response(level)
        }
        DebugSubcommand::Profile => {
            let action = parts.get(1).copied().unwrap_or("start");
            format_profile_response(action)
        }
        DebugSubcommand::Trace => {
            let toggle = parts.get(1).copied().unwrap_or("on");
            let enabled = matches!(toggle.to_lowercase().as_str(), "on" | "true" | "1" | "yes");
            if enabled {
                // SAFETY: REPL event loop is single-threaded; no concurrent reads of SHANNON_TRACE.
                unsafe { std::env::set_var("SHANNON_TRACE", "1"); }
            } else {
                // SAFETY: REPL event loop is single-threaded; no concurrent reads of SHANNON_TRACE.
                unsafe { std::env::remove_var("SHANNON_TRACE"); }
            }
            format_trace_response(enabled)
        }
    };

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

pub(crate) fn handle_doctor(repl: &mut Repl, _args: &str) -> Result<()> {
    use shannon_commands::doctor_utils::{run_all_checks, format_doctor_report};
    let results = run_all_checks();
    let report = format_doctor_report(&results);
    repl.chat.add_message(ChatRole::System, report);
    Ok(())
}

pub(crate) fn handle_diag(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();
    match arg {
        "clear" => {
            repl.state.diagnostic_store.clear();
            repl.chat.add_message(ChatRole::System, "Diagnostics cleared.".to_string());
            return Ok(());
        }
        "" | "check" | "run" => {}
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Usage: /diag [check|clear]\n  /diag      — run diagnostics on current project\n  /diag clear — clear stored diagnostics".to_string());
            return Ok(());
        }
    }

    // Detect project type and run checker
    let cwd = &repl.state.working_directory;
    let (cmd, label) = if std::path::Path::new(cwd).join("Cargo.toml").exists() {
        ("cargo check --message-format=short 2>&1", "cargo check")
    } else if std::path::Path::new(cwd).join("package.json").exists() {
        ("npx tsc --noEmit --pretty false 2>&1 || true", "tsc --noEmit")
    } else if std::path::Path::new(cwd).join("go.mod").exists() {
        ("go vet ./... 2>&1 || true", "go vet")
    } else if std::path::Path::new(cwd).join("pyproject.toml").exists() || std::path::Path::new(cwd).join("setup.py").exists() {
        ("python -m py_compile . 2>&1 || true", "py_compile")
    } else {
        repl.chat.add_message(ChatRole::System,
            "No recognized project found (need Cargo.toml, package.json, go.mod, or pyproject.toml).".to_string());
        return Ok(());
    };

    repl.chat.add_message(ChatRole::System, format!("Running {label}..."));

    // Advisory safety audit: log warnings for suspicious patterns in the
    // diagnostic command.  These commands are hardcoded above, but the audit
    // ensures coverage if the logic is ever extended to accept user input.
    shannon_core::sandbox::audit_shell_command(cmd);

    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            repl.state.diagnostic_store.clear();

            for line in stdout.lines() {
                if let Some(diag) = parse_diag_line(line) {
                    repl.state.diagnostic_store.add(diag);
                }
            }

            let store = &repl.state.diagnostic_store;
            let msg = if store.diagnostics.is_empty() {
                format!("{label}: no issues found.")
            } else {
                let errs = store.error_count();
                let warns = store.warning_count();
                let files = store.diagnostics.iter().map(|d| d.file_path.clone()).collect::<std::collections::HashSet<_>>().len();
                format!("{label}: {} diagnostic(s) across {} file(s) ({} errors, {} warnings)\nUse the sidebar Context tab to view details.",
                    store.diagnostics.len(), files, errs, warns)
            };
            repl.chat.add_message(ChatRole::System, msg);
        }
        Err(e) => {
            super::set_error(repl, &format!("running {label}: {e}"));
        }
    }
    Ok(())
}

/// Parse a diagnostic line from cargo check / tsc / go vet output.
pub(crate) fn parse_diag_line(line: &str) -> Option<crate::lsp_bridge::Diagnostic> {
    use crate::lsp_bridge::DiagnosticSeverity;

    // cargo check format: "crates/foo/src/bar.rs:10:5: error[E0001]: message"
    // tsc format: "src/file.ts(10,5): error TS0001: message"
    // go vet format: "file.go:10: message"

    let line = line.trim();
    if line.is_empty() { return None; }

    // Try cargo/rustc format: path:line:col: severity: message
    if let Some(rest) = line.strip_prefix("error") {
        let severity = if rest.starts_with('[') || rest.starts_with(':') {
            DiagnosticSeverity::Error
        } else {
            return None;
        };
        return parse_path_prefix(line, severity);
    }
    if let Some(rest) = line.strip_prefix("warning") {
        if rest.starts_with('[') || rest.starts_with(':') {
            return parse_path_prefix(line, DiagnosticSeverity::Warning);
        }
    }
    // Try generic path:line:col: format
    if let Some(colon) = line.find(':') {
        let path = &line[..colon];
        if !path.contains('/') && !path.contains('.') { return None; }
        return parse_path_prefix(line, DiagnosticSeverity::Error);
    }
    None
}

pub(crate) fn parse_path_prefix(line: &str, severity: crate::lsp_bridge::DiagnosticSeverity) -> Option<crate::lsp_bridge::Diagnostic> {
    // Find path:line:col: pattern
    let mut parts = line.splitn(4, ':');
    let path = parts.next()?;
    let line_num: usize = parts.next()?.parse().ok()?;
    let _col = parts.next();
    let message = parts.next()?.trim_start_matches(' ').trim_start_matches('[');
    // Clean up error code prefix like E0001]
    let message = message.trim_start_matches(|c: char| c.is_alphanumeric() || c == ']')
        .trim_start_matches(": ")
        .trim_start_matches(']');

    if message.is_empty() { return None; }

    Some(crate::lsp_bridge::Diagnostic {
        severity,
        message: message.to_string(),
        file_path: path.to_string(),
        line: line_num,
        source: Some("check".to_string()),
    })
}
