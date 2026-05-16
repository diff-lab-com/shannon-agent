use crate::{widgets::ChatRole, Result};
use rust_i18n::t;
use shannon_types::recover_lock;
use super::super::Repl;

pub(crate) fn handle_model(repl: &mut Repl, args: &str) -> Result<()> {
    if args.is_empty() {
        let picker = crate::widgets::select::ModelPickerWidget::new(
            repl.state.model.as_deref(),
        );
        repl.state.model_picker = Some(picker);
    } else {
        repl.state.model = Some(args.to_string());
        crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
            model: repl.state.model.clone(),
            provider: repl.state.selected_provider.clone(),
            theme: Some(repl.state.theme.name.to_string()),
        });
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.model.set", name = args).to_string(),
        );
    }
    Ok(())
}

pub(crate) fn handle_init(repl: &mut Repl) -> Result<()> {
    let mut init_info = String::new();
    let cwd = &repl.state.working_directory;

    let is_git = std::path::Path::new(cwd).join(".git").exists();
    if is_git {
        init_info.push_str("Git repository: detected\n");
    } else {
        init_info.push_str("Git repository: not found\n");
    }

    let claude_md_path = std::path::Path::new(cwd).join("CLAUDE.md");
    if claude_md_path.exists() {
        init_info.push_str("CLAUDE.md: already exists\n");
    } else {
        let default_content = "# Project Instructions\n\nThis file contains project-specific instructions for Shannon.\n\n## Coding Standards\n\n- Follow existing code patterns\n- Write clear, descriptive commit messages\n- Keep functions focused and concise\n\n## Project Structure\n\n- Describe your project structure here\n";
        match std::fs::write(&claude_md_path, default_content) {
            Ok(_) => init_info.push_str("CLAUDE.md: created with default template\n"),
            Err(e) => init_info.push_str(&format!("CLAUDE.md: failed to create ({e})\n")),
        }
    }

    init_info.push_str(&format!("Working directory: {cwd}\n"));
    repl.chat.add_message(ChatRole::System, t!("repl.project_initialized", info = init_info).to_string());
    Ok(())
}

pub(crate) fn handle_config(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_tools::config::ConfigManager;
    use shannon_commands::config_utils;

    let mut manager = ConfigManager::new();
    if let Err(e) = manager.load() {
        repl.chat.add_message(ChatRole::System, t!("commands.config.warning_load", error = e).to_string());
    }

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let action_str = parts.first().copied().unwrap_or("");
    let action = config_utils::parse_config_action(action_str);

    let output = match action {
        config_utils::ConfigAction::List => {
            let prefix = if action_str.is_empty() { None } else { parts.get(1).copied() };
            let keys = manager.list(prefix);
            if keys.is_empty() {
                config_utils::format_config_list()
            } else {
                let mut out = config_utils::format_config_list();
                out.push_str(&format!("\nConfig file: {}\n", manager.config_path().display()));
                for key in &keys {
                    let val = manager.get(key).unwrap_or(serde_json::Value::Null);
                    out.push_str(&format!("  {key} = {val}\n"));
                }
                out
            }
        }
        config_utils::ConfigAction::Get => {
            let key = parts.get(1).copied().unwrap_or("");
            if key.is_empty() {
                "Usage: /config get <key>".to_string()
            } else {
                match manager.get(key) {
                    Some(_val) => config_utils::format_config_get(key),
                    None => format!("Config key not found: {key}"),
                }
            }
        }
        config_utils::ConfigAction::Set => {
            let key = parts.get(1).copied().unwrap_or("");
            let value_str = parts.get(2).copied().unwrap_or("");
            if key.is_empty() || value_str.is_empty() {
                "Usage: /config set <key> <value>".to_string()
            } else {
                let value: serde_json::Value = if value_str == "true" {
                    serde_json::json!(true)
                } else if value_str == "false" {
                    serde_json::json!(false)
                } else if let Ok(n) = value_str.parse::<i64>() {
                    serde_json::json!(n)
                } else if let Ok(n) = value_str.parse::<f64>() {
                    serde_json::json!(n)
                } else {
                    serde_json::json!(value_str)
                };
                manager.set(key.to_string(), value.clone());
                match manager.save() {
                    Ok(_) => config_utils::format_config_set(key, &value.to_string()),
                    Err(e) => format!("Error: saving config: {e}"),
                }
            }
        }
        config_utils::ConfigAction::Reset => {
            let key = parts.get(1).copied().unwrap_or("");
            if key.is_empty() {
                "Usage: /config reset <key>".to_string()
            } else {
                let existed = manager.reset(key);
                if existed {
                    let _val = manager.get(key).unwrap_or(serde_json::Value::Null);
                    match manager.save() {
                        Ok(_) => config_utils::format_config_reset(key),
                        Err(e) => format!("Error: saving config: {e}"),
                    }
                } else {
                    config_utils::format_config_reset(key)
                }
            }
        }
        config_utils::ConfigAction::Help => config_utils::format_config_list(),
    };

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

pub(crate) fn handle_mode(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::permissions::ApprovalMode;

    let trimmed = args.trim();

    if trimmed.is_empty() {
        // Show current mode and available options
        let current = {
            let query_engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(ChatRole::System, "Error: Query engine not available.".to_string());
                    return Ok(());
                }
            };
            let permissions = recover_lock(query_engine.permissions().read());
            permissions.approval_mode()
        };
        let mut msg = format!("Current approval mode: {current}\n\nAvailable modes:\n");
        for name in ApprovalMode::all_names() {
            let mode = ApprovalMode::from_str_ci(name).expect("from_str_ci should return valid mode for all_names()");
            let marker = if mode == current { " *" } else { "" };
            msg.push_str(&format!("  {name}{marker} — {}\n", mode.description()));
        }
        {
            repl.chat.add_message(ChatRole::System, msg);
        }
        return Ok(());
    }

    match ApprovalMode::from_str_ci(trimmed) {
        Some(mode) => {
            let query_engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(ChatRole::System, "Error: Query engine not available.".to_string());
                    return Ok(());
                }
            };
            recover_lock(query_engine.permissions().write()).set_approval_mode(mode);
            repl.state.approval_mode_label = mode.short_label().to_string();
            {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Approval mode set to: {mode}\n{}", mode.description()),
                );
            }
            Ok(())
        }
        None => {
            let valid = ApprovalMode::all_names().join(", ");
            {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Unknown mode: '{trimmed}'. Valid modes: {valid}"),
                );
            }
            Ok(())
        }
    }
}

pub(crate) fn handle_context(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    if trimmed == "reload" {
        // Reload project context into the query engine
        let cwd = std::env::current_dir().unwrap_or_default();
        match shannon_core::project_instructions::load_full_context(&cwd) {
            Some(instructions) => {
                let query_engine = match repl.query_engine.as_mut() {
                    Some(e) => e,
                    None => {
                        repl.chat.add_message(ChatRole::System, "Error: Query engine not available.".to_string());
                        return Ok(());
                    }
                };
                query_engine.append_system_prompt(&instructions.content);
                let files = instructions.loaded_files.join(", ");
                {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Project context reloaded. Loaded: {files}"),
                    );
                }
            }
            None => {
                {
                    repl.chat.add_message(
                        ChatRole::System,
                        "No project context found (no CLAUDE.md/AGENTS.md/GEMINI.md and not in a git repo)".to_string(),
                    );
                }
            }
        }
        return Ok(());
    }

    if trimmed == "usage" {
        let total = repl.state.tokens_used;
        let input = repl.state.input_tokens;
        let output = repl.state.output_tokens;
        let cached = repl.state.cache_read_tokens + repl.state.cache_creation_tokens;
        let other = total.saturating_sub(input + output + cached);

        // Build a colored bar using Unicode block chars
        let bar_w = 40usize;
        let max_ctx = 200_000u64; // default context window
        let pct = if total > 0 { (total as f64 / max_ctx as f64).min(1.0) } else { 0.0 };
        let filled = (pct * bar_w as f64).round() as usize;

        let input_w = if total > 0 { (input as f64 / max_ctx as f64 * bar_w as f64).round() as usize } else { 0 };
        let output_w = if total > 0 { (output as f64 / max_ctx as f64 * bar_w as f64).round() as usize } else { 0 };
        let cached_w = if total > 0 { (cached as f64 / max_ctx as f64 * bar_w as f64).round() as usize } else { 0 };
        let other_w = filled.saturating_sub(input_w + output_w + cached_w);

        let mut bar = String::from("[");
        for _ in 0..input_w { bar.push('█'); }
        for _ in 0..output_w { bar.push('▓'); }
        for _ in 0..cached_w { bar.push('░'); }
        for _ in 0..other_w { bar.push('▒'); }
        for _ in 0..(bar_w.saturating_sub(filled)) { bar.push('·'); }
        bar.push(']');

        let fmt_tok = |t: u64| -> String {
            if t < 1000 { format!("{t}") }
            else if t < 1_000_000 { format!("{:.1}k", t as f64 / 1000.0) }
            else { format!("{:.1}M", t as f64 / 1_000_000.0) }
        };

        let mut msg = String::from("Context Window Usage\n\n");
        msg.push_str(&format!("  {} {:.1}%\n\n", bar, pct * 100.0));
        msg.push_str(&format!("  █ Input:    {} tokens\n", fmt_tok(input)));
        msg.push_str(&format!("  ▓ Output:   {} tokens\n", fmt_tok(output)));
        msg.push_str(&format!("  ░ Cached:   {} tokens\n", fmt_tok(cached)));
        if other > 0 {
            msg.push_str(&format!("  ▒ Other:    {} tokens\n", fmt_tok(other)));
        }
        msg.push_str(&format!("  · Free:     {} tokens\n\n", fmt_tok(max_ctx.saturating_sub(total))));
        msg.push_str(&format!("  Total used: {} / {} tokens\n", fmt_tok(total), fmt_tok(max_ctx)));

        if pct > 0.8 {
            msg.push_str("\n  ⚠ Context is over 80% used. Consider /compact to free space.");
        }

        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    // Show current project context info
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut msg = String::from("Project Context:\n\n");

    // Check instruction files
    let instruction_files = ["CLAUDE.md", "AGENTS.md", "GEMINI.md"];
    let mut found_any = false;
    for filename in &instruction_files {
        let path = cwd.join(filename);
        if path.is_file() {
            found_any = true;
            msg.push_str(&format!("  {filename}: found\n"));
        } else {
            msg.push_str(&format!("  {filename}: not found\n"));
        }
    }

    // Check parent directories for instruction files
    let mut current = cwd.parent();
    while let Some(parent) = current {
        for filename in &instruction_files {
            if parent.join(filename).is_file() {
                msg.push_str(&format!("  {filename}: found in {}\n", parent.display()));
                found_any = true;
            }
        }
        current = parent.parent();
    }

    // Git context
    if let Some(git_ctx) = shannon_core::project_instructions::git_context(&cwd) {
        msg.push_str(&format!("\n{git_ctx}"));
        found_any = true;
    } else {
        msg.push_str("\nGit: not a git repository\n");
    }

    if !found_any {
        msg.push_str("\nNo project context available. Create a CLAUDE.md file or initialize a git repo.");
    }

    msg.push_str("\nTip: Use /context reload to refresh the project context.");
    {
        repl.chat.add_message(ChatRole::System, msg);
    }
    Ok(())
}

pub(crate) fn handle_local_models(repl: &mut Repl) -> Result<()> {
    let mut output = String::from("Local Model Detection\n\n");

    // Check Ollama
    let ollama_check = std::process::Command::new("curl")
        .args(["-s", "--connect-timeout", "3", "http://localhost:11434/api/tags"])
        .output();

    match ollama_check {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            if stdout.is_empty() || !result.status.success() {
                output.push_str("Ollama: not running (localhost:11434 unreachable)\n");
            } else {
                output.push_str("Ollama: running at localhost:11434\n");
                // Parse model list
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    if let Some(models) = json.get("models").and_then(|m| m.as_array()) {
                        if models.is_empty() {
                            output.push_str("  No models installed\n");
                        } else {
                            output.push_str(&format!("  Available models ({}):\n", models.len()));
                            for model in models {
                                let name = model.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                                let size = model.get("size").and_then(|s| s.as_u64()).map(|b| format!("{:.1} GB", b as f64 / 1e9)).unwrap_or_default();
                                output.push_str(&format!("    - {name} ({size})\n"));
                            }
                        }
                    }
                } else {
                    output.push_str("  Could not parse model list\n");
                }
            }
        }
        Err(_) => {
            output.push_str("Ollama: not detected (curl not available or host unreachable)\n");
        }
    }

    // Check LM Studio
    let lmstudio_check = std::process::Command::new("curl")
        .args(["-s", "--connect-timeout", "3", "http://localhost:1234/v1/models"])
        .output();

    match lmstudio_check {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            if stdout.is_empty() || !result.status.success() {
                output.push_str("\nLM Studio: not running (localhost:1234 unreachable)\n");
            } else {
                output.push_str("\nLM Studio: running at localhost:1234\n");
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    if let Some(models) = json.get("data").and_then(|m| m.as_array()) {
                        if models.is_empty() {
                            output.push_str("  No models loaded\n");
                        } else {
                            output.push_str(&format!("  Loaded models ({}):\n", models.len()));
                            for model in models {
                                let id = model.get("id").and_then(|i| i.as_str()).unwrap_or("unknown");
                                output.push_str(&format!("    - {id}\n"));
                            }
                        }
                    }
                }
            }
        }
        Err(_) => {
            output.push_str("\nLM Studio: not detected\n");
        }
    }

    // Suggest usage
    output.push_str("\nTo use a local model:\n");
    output.push_str("  /model ollama/llama3\n");
    output.push_str("  /model ollama/mistral\n");
    output.push_str("  /model lmstudio/<model-id>\n");
    output.push_str(&format!("\nCurrent model: {}\n", repl.state.model.as_deref().unwrap_or("not set")));

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

/// /theme — switch color theme or list available themes.
pub(crate) fn handle_theme(repl: &mut Repl, args: &str) -> Result<()> {
    use crate::theme::Theme;

    let args = args.trim();

    if args == "pick" || args == "picker" || args == "preview" {
        let themes = Theme::available();
        let current = &repl.state.theme.name;
        let items: Vec<_> = themes.into_iter().map(|name| {
            let label = if name == *current {
                format!("{name} (current)")
            } else {
                name.clone()
            };
            crate::widgets::select::SelectItem::new(label, name)
        }).collect();

        let picker = crate::widgets::select::FuzzyPickerWidget::new("Theme Picker".to_string())
            .with_items(items);
        repl.state.theme_picker = Some(picker);
        return Ok(());
    }

    if args.is_empty() || args == "list" {
        let current = &repl.state.theme.name;
        let available = Theme::available();
        let mut msg = String::from("Available themes:\n");
        for name in available {
            if name == *current {
                msg.push_str(&format!("  * {name} (current)\n"));
            } else {
                msg.push_str(&format!("    {name}\n"));
            }
        }
        msg.push_str("\nUsage: /theme <name>");
        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    match Theme::named(args) {
        Some(theme) => {
            let name = theme.name.clone();
            repl.renderer.set_theme(&theme);
            repl.state.theme = theme;
            crate::repl::preferences::save_preferences(
                &crate::repl::preferences::Preferences {
                    model: repl.state.model.clone(),
                    provider: repl.state.selected_provider.clone(),
                    theme: Some(name.to_string()),
                },
            );
            repl.chat.add_message(
                ChatRole::System,
                format!("Theme switched to '{name}'."),
            );
        }
        None => {
            let available = Theme::available().join(", ");
            repl.chat.add_message(
                ChatRole::System,
                format!("Unknown theme '{args}'. Available: {available}"),
            );
        }
    }

    Ok(())
}

/// /accessibility — toggle or check accessibility mode.
pub(crate) fn handle_accessibility(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();
    match arg {
        "on" | "enable" | "true" | "1" => {
            repl.state.accessibility_mode = true;
            crate::a11y::set_enabled(true);
            repl.chat.add_message(ChatRole::System,
                "Accessibility mode enabled. Decorative characters replaced with plain text.".to_string());
        }
        "off" | "disable" | "false" | "0" => {
            repl.state.accessibility_mode = false;
            crate::a11y::set_enabled(false);
            repl.chat.add_message(ChatRole::System,
                "Accessibility mode disabled.".to_string());
        }
        "" | "status" => {
            let state = if repl.state.accessibility_mode { "enabled" } else { "disabled" };
            repl.chat.add_message(ChatRole::System,
                format!("Accessibility mode: {state}\n\nUsage: /accessibility on|off\nAlso auto-enabled via NO_GRAPHICS or ACCESSIBILITY env vars."));
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Usage: /accessibility on|off|status".to_string());
        }
    }
    Ok(())
}

pub(crate) fn handle_terminal_setup(repl: &mut Repl) -> Result<()> {
    let mut report = String::from("Terminal Setup Check\n\n");

    // 1. Shell detection
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string());
    let shell_name = std::path::Path::new(&shell)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| shell.clone());
    report.push_str(&format!("Shell: {shell_name} ({shell})\n"));

    // 2. Terminal type
    let term = std::env::var("TERM").unwrap_or_else(|_| "not set".to_string());
    report.push_str(&format!("TERM: {term}\n"));

    // 3. Check if shannon is on PATH
    let shannon_on_path = std::process::Command::new("which")
        .arg("shannon")
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    report.push_str(&format!(
        "shannon on PATH: {}\n",
        if shannon_on_path { "yes" } else { "no — add shannon to your PATH" }
    ));

    // 4. Check for common terminal tools
    for tool in &["git", "gh", "node"] {
        let found = std::process::Command::new("which")
            .arg(tool)
            .output()
            .ok()
            .map(|o| o.status.success())
            .unwrap_or(false);
        report.push_str(&format!(
            "{tool}: {}\n",
            if found { "found" } else { "not found" }
        ));
    }

    // 5. Check shell integration markers
    // Claude Code uses SHANNON_INTEGRATION_DIR or similar env vars
    let has_integration = std::env::var("SHANNON_SHELL_INTEGRATION")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    report.push_str(&format!(
        "Shell integration: {}\n",
        if has_integration {
            "active"
        } else {
            "not detected — add `eval \"$(shannon init)\"` to your shell profile for inline diagnostics and key bindings"
        }
    ));

    // 6. Check terminal dimensions
    let (w, h) = crossterm::terminal::size().unwrap_or((0, 0));
    report.push_str(&format!("Terminal size: {w}x{h}\n"));
    if w < 80 {
        report.push_str("  ⚠ Terminal width < 80 columns — UI may be cramped\n");
    }

    // 7. Color support
    let colors = std::env::var("COLORTERM").unwrap_or_else(|_| "not set".to_string());
    report.push_str(&format!("COLORTERM: {colors}\n"));

    // 8. Key binding hint
    report.push_str("\nKey bindings:\n");
    report.push_str("  Enter      — submit input\n");
    report.push_str("  Ctrl+C     — cancel current operation\n");
    report.push_str("  Ctrl+D     — exit Shannon\n");
    report.push_str("  Tab        — autocomplete\n");
    report.push_str("  Up/Down    — navigate history\n");
    report.push_str("  Escape     — enter/exit vim normal mode\n");

    report.push_str("\nShell profile setup:\n");
    match shell_name.as_str() {
        "zsh" => report.push_str("  Add to ~/.zshrc:\n    eval \"$(shannon init zsh)\"\n"),
        "bash" => report.push_str("  Add to ~/.bashrc:\n    eval \"$(shannon init bash)\"\n"),
        "fish" => report.push_str("  Add to ~/.config/fish/config.fish:\n    shannon init fish | source\n"),
        other => report.push_str(&format!("  Unknown shell '{other}'. Add the appropriate init line to your shell profile.\n")),
    }

    repl.chat.add_message(ChatRole::System, report);
    Ok(())
}

/// Handle /color command — set prompt bar color per session
pub(crate) fn handle_color(repl: &mut Repl, args: &str) -> Result<()> {
    let color = args.trim();
    if color.is_empty() || color == "default" || color == "reset" {
        repl.state.prompt_bar_color = None;
        repl.prompt.set_border_color(None);
        repl.chat.add_message(ChatRole::System, "Prompt bar color reset to default.".to_string());
    } else {
        // Validate color by trying to parse it
        let parsed = parse_color_string(color);
        match parsed {
            Some(c) => {
                repl.state.prompt_bar_color = Some(color.to_string());
                repl.prompt.set_border_color(Some(c));
                repl.chat.add_message(ChatRole::System, format!("Prompt bar color set to {color}."));
            }
            None => {
                repl.chat.add_message(ChatRole::System, format!(
                    "Unknown color: \"{color}\". Use a named color (red, green, blue, ...) or hex (#ff0000), or \"default\" to reset."
                ));
            }
        }
    }
    Ok(())
}

/// Parse a color string into a ratatui Color
pub(crate) fn handle_statusline(repl: &mut Repl, args: &str) -> Result<()> {
    let cmd = args.trim();
    if cmd.is_empty() || cmd == "off" || cmd == "reset" || cmd == "default" {
        repl.state.statusline_command = None;
        repl.state.cached_statusline = None;
        repl.chat.add_message(ChatRole::System, "Custom statusline disabled.".to_string());
    } else {
        repl.state.statusline_command = Some(cmd.to_string());
        repl.state.cached_statusline = None;
        repl.state.statusline_last_update = None;
        repl.chat.add_message(ChatRole::System, format!("Custom statusline set to: {cmd}"));
    }
    Ok(())
}

fn parse_color_string(s: &str) -> Option<ratatui::style::Color> {
    use ratatui::style::Color;
    let lower = s.to_lowercase();
    match lower.as_str() {
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "blue" => Some(Color::Blue),
        "yellow" => Some(Color::Yellow),
        "magenta" | "purple" | "pink" => Some(Color::Magenta),
        "cyan" | "teal" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "dark_grey" | "darkgrey" => Some(Color::DarkGray),
        "lightred" | "light_red" => Some(Color::LightRed),
        "lightgreen" | "light_green" => Some(Color::LightGreen),
        "lightblue" | "light_blue" => Some(Color::LightBlue),
        "lightyellow" | "light_yellow" => Some(Color::LightYellow),
        "lightmagenta" | "light_magenta" => Some(Color::LightMagenta),
        "lightcyan" | "light_cyan" => Some(Color::LightCyan),
        "black" => Some(Color::Black),
        _ => {
            // Try hex color
            let hex = s.trim_start_matches('#');
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color::Rgb(r, g, b))
            } else {
                None
            }
        }
    }
}

pub(crate) fn handle_lang(repl: &mut Repl, args: &str) -> Result<()> {
    let supported = ["en", "zh", "hi", "es", "fr", "ar", "bn", "pt", "ru", "ja"];
    let input = args.trim();

    if input.is_empty() {
        let current = shannon_core::i18n::current_locale();
        repl.chat.add_message(ChatRole::System, format!(
            "Current language: {current}\n\nUsage: /lang <code>\nSupported: {}",
            supported.join(", ")
        ));
        return Ok(());
    }

    let lang = input.to_lowercase();
    if supported.contains(&lang.as_str()) {
        shannon_core::i18n::set_locale(&lang);
        repl.chat.add_message(ChatRole::System, format!("Language set to: {lang}"));
    } else {
        repl.chat.add_message(ChatRole::System, format!(
            "Unsupported language: {lang}\nSupported: {}",
            supported.join(", ")
        ));
    }
    Ok(())
}
