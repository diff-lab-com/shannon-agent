//! Loop engine command handlers: /loop, /ralph, /routine, /bind, /project, /agent, /stats,
//! /sandbox, /notify, and related helpers.

use crate::{widgets::ChatRole, Result};

use super::super::Repl;

/// Handle `/loop` command — autonomous iteration engine.
///
/// Usage:
///   /loop <task>           — start loop with task description
///   /loop --max N <task>   — limit to N iterations
///   /loop stop             — stop the current loop
///   /loop status           — show current loop state
pub(crate) fn handle_loop(repl: &mut Repl, args: &str) -> Result<()> {
    let input = args.trim();

    if input == "stop" || input == "cancel" {
        if let Some(ref mut ls) = repl.state.loop_state {
            ls.active = false;
            let iter = ls.iteration;
            repl.chat.add_message(ChatRole::System, format!(
                "Loop stopped after {iter} iteration(s)."
            ));
        } else {
            repl.chat.add_message(ChatRole::System, "No active loop to stop.".to_string());
        }
        repl.state.loop_state = None;
        return Ok(());
    }

    if input == "status" {
        if let Some(ref ls) = repl.state.loop_state {
            repl.chat.add_message(ChatRole::System, format!(
                "Loop active: iteration {}/{}\nTask: {}",
                ls.iteration,
                if ls.max_iterations == 0 { "unlimited".to_string() } else { ls.max_iterations.to_string() },
                ls.task,
            ));
        } else {
            repl.chat.add_message(ChatRole::System, "No active loop.".to_string());
        }
        return Ok(());
    }

    if input.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage:\n  /loop <task>         — start autonomous iteration\n  /loop --max N <task> — limit to N iterations\n  /loop stop           — stop current loop\n  /loop status         — show loop state".to_string()
        );
        return Ok(());
    }

    // Parse --max N
    let (max_iter, task) = if input.starts_with("--max ") {
        let rest = input.strip_prefix("--max ").unwrap_or("");
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let n: usize = parts.first().unwrap_or(&"0").parse().unwrap_or(0);
        let t = parts.get(1).copied().unwrap_or("").trim();
        (n, t.to_string())
    } else {
        (0, input.to_string())
    };

    if task.is_empty() {
        super::set_error(repl, "no task description provided");
        return Ok(());
    }

    // Set up loop state
    repl.state.loop_state = Some(super::super::LoopState {
        task: task.clone(),
        max_iterations: max_iter,
        iteration: 0,
        active: true,
    });

    repl.chat.add_message(ChatRole::System, format!(
        "Loop started{}.\nTask: {task}\nType /loop stop to cancel.",
        if max_iter > 0 { format!(" (max {max_iter} iterations)") } else { String::new() }
    ));

    // Trigger first iteration
    let prompt = format!(
        "[Loop iteration 1] Task: {task}\n\nPlease work on this task. After completing, summarize what you did and what remains."
    );
    repl.prompt.set_input(prompt);
    super::submit_input(repl, None)?;

    Ok(())
}

/// Called after a query completes. If a loop is active, triggers the next iteration.
/// Returns true if a new loop iteration was started.
pub(crate) fn check_loop_iteration(repl: &mut Repl) -> bool {
    let should_continue = repl.state.loop_state.as_ref().is_some_and(|ls| ls.active);
    if !should_continue {
        return false;
    }

    let ls = match repl.state.loop_state.as_mut() {
        Some(ls) => ls,
        None => return false,
    };
    ls.iteration += 1;

    // Check max iterations
    if ls.max_iterations > 0 && ls.iteration >= ls.max_iterations {
        let iter = ls.iteration;
        repl.chat.add_message(ChatRole::System, format!(
            "Loop completed: reached max {iter} iteration(s)."
        ));
        repl.state.loop_state = None;
        return false;
    }

    let task = ls.task.clone();
    let iter = ls.iteration + 1;

    let prompt = format!(
        "[Loop iteration {iter}] Continuing task: {task}\n\nReview what was done in the previous iteration and continue working. Summarize progress and what remains."
    );
    repl.prompt.set_input(prompt);

    // Submit next iteration
    if super::submit_input(repl, None).is_err() {
        repl.state.loop_state = None;
        return false;
    }

    true
}

/// Handle the `/ralph` command — completion-based loop that re-injects
/// the task prompt until the model emits a completion keyword.
///
/// Usage:
///   /ralph <task>                  — start with defaults (max 10, keywords: DONE, FIXED, COMPLETE, RESOLVED, ALL TESTS PASS)
///   /ralph --max N <task>          — limit to N iterations
///   /ralph --done KEYWORD <task>   — custom completion keyword (can be repeated)
///   /ralph stop                    — stop the current ralph loop
///   /ralph status                  — show current ralph state
pub(crate) fn handle_ralph(repl: &mut Repl, args: &str) -> Result<()> {
    let input = args.trim();

    if input == "stop" || input == "cancel" {
        if let Some(ref rs) = repl.state.ralph_state {
            let iter = rs.iteration;
            repl.chat.add_message(ChatRole::System, format!(
                "Ralph stopped after {iter} iteration(s)."
            ));
        } else {
            repl.chat.add_message(ChatRole::System, "No active ralph loop to stop.".to_string());
        }
        repl.state.ralph_state = None;
        return Ok(());
    }

    if input == "status" {
        if let Some(ref rs) = repl.state.ralph_state {
            repl.chat.add_message(ChatRole::System, format!(
                "Ralph active: iteration {}/{}\nKeywords: {}\nTask: {}",
                rs.iteration,
                rs.max_iterations,
                rs.completion_keywords.join(", "),
                rs.task,
            ));
        } else {
            repl.chat.add_message(ChatRole::System, "No active ralph loop.".to_string());
        }
        return Ok(());
    }

    if input.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage:\n  /ralph <task>              — start completion-based loop\n  /ralph --max N <task>      — limit to N iterations\n  /ralph --done KEYWORD <task> — custom completion keyword\n  /ralph stop                 — stop current loop\n  /ralph status               — show loop state".to_string()
        );
        return Ok(());
    }

    // Parse flags
    let mut max_iter: usize = 10;
    let mut keywords: Vec<String> = vec![
        "DONE".into(), "FIXED".into(), "COMPLETE".into(),
        "RESOLVED".into(), "ALL TESTS PASS".into(),
    ];
    let mut remaining = input;

    // Parse --max N
    if let Some(rest) = remaining.strip_prefix("--max ") {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        max_iter = parts.first().unwrap_or(&"10").parse().unwrap_or(10);
        remaining = parts.get(1).copied().unwrap_or("").trim();
    }

    // Parse --done KEYWORD (possibly multiple)
    while let Some(rest) = remaining.strip_prefix("--done ") {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        if let Some(kw) = parts.first() {
            keywords = vec![kw.to_uppercase()]; // custom replaces defaults
        }
        remaining = parts.get(1).copied().unwrap_or("").trim();
    }

    let task = remaining.trim().to_string();
    if task.is_empty() {
        super::set_error(repl, "no task description provided");
        return Ok(());
    }

    // Set up ralph state
    repl.state.ralph_state = Some(super::super::RalphState {
        task: task.clone(),
        completion_keywords: keywords.clone(),
        max_iterations: max_iter,
        iteration: 0,
        active: true,
    });

    repl.chat.add_message(ChatRole::System, format!(
        "Ralph started (max {max_iter} iterations).\nKeywords: {}\nTask: {task}\nType /ralph stop to cancel.",
        keywords.join(", ")
    ));

    // Trigger first iteration
    let prompt = format!(
        "[Ralph iteration 1] Task: {task}\n\n\
         Work on this task. When you are truly done, output one of these keywords on its own line: {}\n\
         If you are not done, keep working. Do NOT output a completion keyword unless the task is fully complete.",
        keywords.join(", ")
    );
    repl.prompt.set_input(prompt);
    super::submit_input(repl, None)?;

    Ok(())
}

/// Called after a query completes. If a ralph loop is active, checks the
/// last assistant message for completion keywords and either stops or
/// re-injects the task prompt.
///
/// Returns true if a new ralph iteration was started.
pub(crate) fn check_ralph_iteration(repl: &mut Repl) -> bool {
    let should_continue = repl.state.ralph_state.as_ref().is_some_and(|rs| rs.active);
    if !should_continue {
        return false;
    }

    let rs = match repl.state.ralph_state.as_mut() {
        Some(rs) => rs,
        None => return false,
    };
    rs.iteration += 1;

    // Get last assistant message to check for completion keywords
    let last_msg = repl.chat.last_message().map(|m| m.content.to_uppercase());
    let keywords = rs.completion_keywords.clone();

    if let Some(ref msg) = last_msg {
        let found = keywords.iter().any(|kw| msg.contains(&kw.to_uppercase()));
        if found {
            let iter = rs.iteration;
            let matched_kw = keywords.iter().find(|kw| msg.contains(&kw.to_uppercase())).unwrap_or(&keywords[0]);
            repl.chat.add_message(ChatRole::System, format!(
                "Ralph complete: detected \"{matched_kw}\" after {iter} iteration(s)."
            ));
            repl.state.ralph_state = None;
            return false;
        }
    }

    // Check max iterations
    if rs.iteration >= rs.max_iterations {
        let iter = rs.iteration;
        repl.chat.add_message(ChatRole::System, format!(
            "Ralph stopped: reached max {iter} iteration(s) without completion keyword."
        ));
        repl.state.ralph_state = None;
        return false;
    }

    let task = rs.task.clone();
    let iter = rs.iteration + 1;

    let prompt = format!(
        "[Ralph iteration {iter}] Continuing task: {task}\n\n\
         The task is NOT yet complete — no completion keyword was detected.\n\
         Keep working. When truly done, output one of these on its own line: {}\n\
         Summarize what was done and what remains.",
        keywords.join(", ")
    );
    repl.prompt.set_input(prompt);

    if super::submit_input(repl, None).is_err() {
        repl.state.ralph_state = None;
        return false;
    }

    true
}

/// Check if platform sandbox (bwrap/seatbelt) is available.
pub(crate) fn detect_platform_sandbox() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new("/usr/bin/bwrap").exists() || which_exists("bwrap") {
            return "bubblewrap (bwrap) available";
        }
    }
    #[cfg(target_os = "macos")]
    {
        if which_exists("sandbox-exec") {
            return "seatbelt (sandbox-exec) available";
        }
    }
    "no platform sandbox detected"
}

/// Simple check if a command exists in PATH.
pub(crate) fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn default_keybindings() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Enter", "Submit input / confirm dialog"),
        ("Shift+Enter", "Insert newline"),
        ("Tab", "Autocomplete / cycle suggestions"),
        ("Ctrl+C", "Cancel current operation"),
        ("Ctrl+P", "Open command palette"),
        ("Ctrl+L", "Clear screen"),
        ("Ctrl+R", "Search history"),
        ("Up/Down", "Navigate history / move cursor (multiline)"),
        ("Left/Right", "Move cursor"),
        ("Home/End", "Move to start/end of line"),
        ("Ctrl+U", "Clear input line"),
        ("Ctrl+W", "Delete word backward"),
        ("Ctrl+A", "Move to start of line"),
        ("Ctrl+E", "Move to end of line"),
        ("Ctrl+K", "Delete to end of line"),
        ("Esc", "Cancel / dismiss dialog"),
        ("Page Up/Down", "Scroll chat"),
    ]
}

pub(crate) fn handle_bind(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    if trimmed.is_empty() || trimmed == "list" || trimmed == "show" {
        let mut msg = "Keyboard Shortcuts\n\n".to_string();
        msg.push_str("  Key              Action\n");
        msg.push_str("  ──────────────── ─────────────────────────────────\n");
        for (key, action) in default_keybindings() {
            msg.push_str(&format!("  {key:<16} {action}\n"));
        }
        msg.push_str("\nCustom keybindings can be set in ~/.shannon/keybindings.toml\n");
        msg.push_str("Format: [[bind]]\n  key = \"Ctrl+J\"\n  action = \"submit\"\n");
        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    if trimmed == "save" {
        let config_dir = dirs::home_dir()
            .map(|h| h.join(".shannon"))
            .unwrap_or_else(|| std::path::PathBuf::from(".shannon"));
        let _ = std::fs::create_dir_all(&config_dir);
        let kb_path = config_dir.join("keybindings.toml");

        let mut toml_content = "# Shannon keybindings configuration\n".to_string();
        toml_content.push_str("# Restart Shannon after modifying this file.\n\n");
        for (key, action) in default_keybindings() {
            toml_content.push_str(&format!("# {key}: {action}\n"));
        }
        toml_content.push_str("\n# Example custom binding:\n");
        toml_content.push_str("# [[bind]]\n# key = \"Ctrl+J\"\n# action = \"submit\"\n");

        match std::fs::write(&kb_path, &toml_content) {
            Ok(()) => {
                repl.chat.add_message(ChatRole::System,
                    format!("Keybindings template saved to {}", kb_path.display()));
            }
            Err(e) => {
                super::set_error(repl, &format!("saving keybindings: {e}"));
            }
        }
        return Ok(());
    }

    let kb_path = dirs::home_dir()
        .map(|h| h.join(".shannon").join("keybindings.toml"))
        .unwrap_or_else(|| std::path::PathBuf::from(".shannon/keybindings.toml"));

    if trimmed == "load" || trimmed == "reload" {
        if !kb_path.exists() {
            repl.chat.add_message(ChatRole::System,
                "No custom keybindings file found. Use /bind save to create one.".to_string());
        } else {
            match std::fs::read_to_string(&kb_path) {
                Ok(content) => {
                    let line_count = content.lines().filter(|l| l.starts_with("[[bind]]")).count();
                    repl.chat.add_message(ChatRole::System,
                        format!("Loaded keybindings config ({line_count} custom binding(s) defined).\nKeybindings take effect on next restart."));
                }
                Err(e) => {
                    super::set_error(repl, &format!("reading keybindings: {e}"));
                }
            }
        }
        return Ok(());
    }

    repl.chat.add_message(ChatRole::System,
        "Usage: /bind [list|save|load]\n  /bind       — Show all keybindings\n  /bind save  — Save template to ~/.shannon/keybindings.toml\n  /bind load  — Reload custom keybindings".to_string());
    Ok(())
}

pub(crate) fn handle_project(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    if trimmed.is_empty() || trimmed == "status" || trimmed == "show" {
        let cwd = &repl.state.working_directory;
        let mut msg = format!("Project Configuration\n\n  Directory: {cwd}\n");

        let config_files = [".shannon.toml", "CLAUDE.md", "AGENTS.md", "GEMINI.md", ".claude/settings.json"];
        msg.push_str("\n  Config files:\n");
        for file in &config_files {
            let path = std::path::Path::new(cwd).join(file);
            if path.exists() {
                msg.push_str(&format!("    + {file} (found)\n"));
            } else {
                msg.push_str(&format!("    - {file}\n"));
            }
        }

        if let Some(ref model) = repl.state.model {
            msg.push_str(&format!("\n  Model: {model}"));
        }

        msg.push_str(&format!("\n  Sandbox: {:?}", repl.state.sandbox_mode));

        if let Some(ref engine) = repl.query_engine {
            let perms = engine.permissions();
            let mode = perms.read().map(|p| p.approval_mode()).unwrap_or(shannon_core::permissions::ApprovalMode::Suggest);
            msg.push_str(&format!("\n  Permission mode: {mode:?}"));
        }

        if repl.state.plan.active {
            msg.push_str("\n  Plan mode: active");
        }

        msg.push_str(&format!("\n  Notifications: {}", if repl.notifications_enabled { "enabled" } else { "disabled" }));

        let git_check = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(cwd)
            .output();
        if let Ok(output) = git_check {
            if output.status.success() {
                let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
                msg.push_str(&format!("\n  Git root: {root}"));
            }
        }

        if let Some(ref engine) = repl.query_engine {
            msg.push_str(&format!("\n  Tools loaded: {}", engine.tools().list().len()));
        }

        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    if trimmed == "init" {
        let config_path = std::path::Path::new(&repl.state.working_directory).join(".shannon.toml");
        if config_path.exists() {
            repl.chat.add_message(ChatRole::System,
                format!("Project config already exists: {}", config_path.display()));
            return Ok(());
        }

        let template = "# Shannon project configuration\n\
[project]\n\
name = \"\"\n\
description = \"\"\n\
\n\
[model]\n\
default = \"claude-3-5-sonnet\"\n\
\n\
[tools]\n\
allowed = []        # Empty = all tools allowed\n\
denied = []         # Explicit deny list\n\
\n\
[sandbox]\n\
mode = \"direct\"     # direct | docker\n\
\n\
[context]\n\
auto_load = true    # Auto-load CLAUDE.md / AGENTS.md\n\
max_files = 20      # Max files for /add glob\n\
\n\
[permissions]\n\
mode = \"suggest\"    # suggest | auto-edit | full-auto | readonly\n\
\n\
[routes]\n\
# Pattern-based model routing\n\
# \"translate\" = \"claude-3-5-haiku\"\n\
# \"review\" = \"claude-3-5-sonnet\"\n";

        match std::fs::write(&config_path, template) {
            Ok(()) => {
                repl.chat.add_message(ChatRole::System,
                    format!("Created project config: {}\nEdit it to customize Shannon for this project.", config_path.display()));
            }
            Err(e) => {
                super::set_error(repl, &format!("creating config: {e}"));
            }
        }
        return Ok(());
    }

    if let Some(rest) = trimmed.strip_prefix("model ") {
        let model = rest.trim();
        if model.is_empty() {
            repl.chat.add_message(ChatRole::System,
                format!("Current model: {}", repl.state.model.as_deref().unwrap_or("none")));
        } else {
            repl.state.model = Some(model.to_string());
            crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
                model: repl.state.model.clone(),
                provider: repl.state.selected_provider.clone(),
                theme: Some(repl.state.theme.name.to_string()),
            });
            repl.chat.add_message(ChatRole::System,
                format!("Project model set to: {model}"));
        }
        return Ok(());
    }

    if let Some(rest) = trimmed.strip_prefix("set ") {
        let rest = rest.trim();
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        if parts.len() < 2 {
            repl.chat.add_message(ChatRole::System,
                "Usage: /project set <key> <value>\nKeys: model, sandbox, permissions, notifications".to_string());
            return Ok(());
        }
        let key = parts[0];
        let value = parts[1];

        match key {
            "sandbox" => {
                repl.state.sandbox_mode = shannon_tools::SandboxMode::from_str_loose(value);
                repl.chat.add_message(ChatRole::System,
                    format!("Sandbox mode set to: {:?}", repl.state.sandbox_mode));
            }
            "permissions" => {
                let mode = match value {
                    "auto-edit" => shannon_core::permissions::ApprovalMode::AutoEdit,
                    "full-auto" => shannon_core::permissions::ApprovalMode::FullAuto,
                    "readonly" => shannon_core::permissions::ApprovalMode::Readonly,
                    _ => shannon_core::permissions::ApprovalMode::Suggest,
                };
                if let Some(ref engine) = repl.query_engine {
                    if let Ok(mut perms) = engine.permissions().write() {
                        perms.set_approval_mode(mode);
                    }
                    repl.state.approval_mode_label = mode.short_label().to_string();
                }
                repl.chat.add_message(ChatRole::System,
                    format!("Permission mode set to: {value}"));
            }
            "notifications" => {
                repl.notifications_enabled = value == "on" || value == "true" || value == "enabled";
                repl.chat.add_message(ChatRole::System,
                    format!("Notifications: {}", if repl.notifications_enabled { "enabled" } else { "disabled" }));
            }
            _ => {
                repl.chat.add_message(ChatRole::System,
                    format!("Unknown setting: {key}. Available: model, sandbox, permissions, notifications"));
            }
        }
        return Ok(());
    }

    repl.chat.add_message(ChatRole::System,
        "Usage: /project [status|init|model <name>|set <key> <value>]\n\
         /project status  — Show current project config\n\
         /project init    — Create .shannon.toml template\n\
         /project model <name> — Set project model\n\
         /project set <key> <value> — Set config value".to_string());
    Ok(())
}

pub(crate) fn handle_stats(repl: &mut Repl) -> Result<()> {
    repl.state.sidebar_tab = crate::repl::SidebarTab::Perf;
    let dur = repl.state.session_start.map(|t| t.elapsed().as_secs()).unwrap_or(0);
    let tok = repl.state.tokens_used;
    let turns = repl.current_turn;
    let cost = repl.state.total_cost_usd;
    let tools = repl.tools_invoked;
    let cmds = repl.commands_run;
    let tps = if dur > 0 && tok > 0 {
        format!("{:.0} tok/s", tok as f64 / dur as f64)
    } else {
        "N/A".to_string()
    };
    let dur_str = if dur >= 3600 {
        format!("{}h {}m", dur / 3600, (dur % 3600) / 60)
    } else if dur >= 60 {
        format!("{}m {}s", dur / 60, dur % 60)
    } else {
        format!("{dur}s")
    };
    let model = repl.state.model.as_deref().unwrap_or("unknown");
    repl.chat.add_message(ChatRole::System, format!(
        "Performance stats (switched to Perf tab):\n  Model: {model}\n  Duration: {dur_str}\n  Tokens: {tok} ({tps})\n  Turns: {turns}\n  Cost: ${cost:.4}\n  Tools: {tools} | Commands: {cmds}"
    ));
    Ok(())
}

pub(crate) fn handle_sandbox(repl: &mut Repl, args: &str) -> Result<()> {
    let args = args.trim();

    if args.is_empty() || args == "--help" || args == "help" {
        let docker_available = repl.runtime.block_on(
            shannon_tools::DockerSandbox::is_available()
        );
        let status = if docker_available { "available" } else { "not installed/unavailable" };
        let platform = detect_platform_sandbox();

        repl.chat.add_message(ChatRole::System,
            "Sandbox — execution isolation for shell commands\n\n\
             Usage:\n\
               /sandbox              Show current sandbox status\n\
               /sandbox status       Show detailed sandbox info\n\
               /sandbox docker       Enable Docker isolation\n\
               /sandbox direct       Disable sandbox (run directly)\n\
               /sandbox check        Check if Docker is available\n\n\
             Docker: ".to_string() + status + "\n\
             Platform: " + platform + "\n\n\
             When Docker sandbox is enabled, all /bash tool commands\n\
             run inside an isolated container with:\n\
               - No network access (network=none)\n\
               - Memory limit (512m)\n\
               - CPU limit (1.0)\n\
               - Read-only root filesystem\n\
               - Workspace mounted at /workspace"
        );
        return Ok(());
    }

    match args {
        "status" | "info" => {
            let current = repl.state.sandbox_mode.clone();
            let mode_str = match &current {
                shannon_tools::SandboxMode::Direct => "direct (no sandbox)".to_string(),
                shannon_tools::SandboxMode::Docker(cfg) => {
                    format!("docker (image={}, network={}, memory={}, cpus={})",
                        cfg.image,
                        cfg.network,
                        cfg.memory.as_deref().unwrap_or("unlimited"),
                        cfg.cpus.as_deref().unwrap_or("unlimited"),
                    )
                }
            };
            repl.chat.add_message(ChatRole::System,
                format!("Sandbox mode: {mode_str}"));
        }
        "docker" | "on" | "enable" => {
            let config = shannon_tools::DockerSandboxConfig::default();
            repl.state.sandbox_mode = shannon_tools::SandboxMode::Docker(config);
            repl.chat.add_message(ChatRole::System,
                "Docker sandbox enabled. Shell commands will run inside an isolated container.\n\
                 Use /sandbox status for details, /sandbox direct to disable.".to_string());
        }
        "direct" | "off" | "disable" => {
            repl.state.sandbox_mode = shannon_tools::SandboxMode::Direct;
            repl.chat.add_message(ChatRole::System,
                "Sandbox disabled. Shell commands will run directly on the host.".to_string());
        }
        "check" => {
            let available = repl.runtime.block_on(
                shannon_tools::DockerSandbox::is_available()
            );
            if available {
                repl.chat.add_message(ChatRole::System,
                    "Docker is available and running.".to_string());
            } else {
                repl.chat.add_message(ChatRole::System,
                    "Docker is not available. Install Docker and ensure the daemon is running.".to_string());
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                format!("Unknown sandbox option: {args}\n\
                 Use: /sandbox [status|docker|direct|check]"));
        }
    }

    Ok(())
}

#[allow(dead_code)]
pub(crate) fn handle_notify(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    match trimmed {
        "on" | "enable" | "true" | "yes" => {
            repl.notifications_enabled = true;
            repl.chat.add_message(
                ChatRole::System,
                "Desktop notifications enabled. You'll be notified when queries complete.".to_string(),
            );
        }
        "off" | "disable" | "false" | "no" => {
            repl.notifications_enabled = false;
            repl.chat.add_message(
                ChatRole::System,
                "Desktop notifications disabled.".to_string(),
            );
        }
        "test" => {
            repl.notifier.info("Shannon", "Test notification!").ok();
            repl.chat.add_message(
                ChatRole::System,
                "Test notification sent.".to_string(),
            );
        }
        _ => {
            let status = if repl.notifications_enabled { "enabled" } else { "disabled" };
            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "Desktop notifications: {status}\n\n\
                     Usage:\n  /notify on  — enable notifications\n  \
                     /notify off — disable notifications\n  \
                     /notify test — send a test notification"
                ),
            );
        }
    }
    Ok(())
}

/// Send a desktop notification if enabled.
pub(crate) fn notify_query_complete(notifier: &shannon_core::notifier::Notifier, enabled: bool, message: &str) {
    if !enabled { return; }
    let notification = shannon_core::notifier::Notification {
        title: "Shannon - Query Complete".to_string(),
        body: message.to_string(),
        level: shannon_core::notifier::NotificationLevel::Info,
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
    };
    let _ = notifier.notify(&notification);
}

/// Handle custom agent definition commands
pub(crate) fn handle_agent(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_agents::agent_defs::AgentDefinitionRegistry;
    use shannon_agents::custom_agent::CustomAgentLoader;
    use std::path::PathBuf;

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("help");

    match subcommand {
        "help" | "" => {
            repl.chat.add_message(ChatRole::System, "\
/agent list                    — List all available agent definitions
/agent run <name> [prompt]     — Run an agent with optional prompt
/agent create <name>           — Interactive agent creation wizard
/agent edit <name>             — Edit an agent definition
/agent show <name>             — Show agent definition details

Agent definitions are loaded from:
  .claude/agents/*.md  (project-local, highest priority)
  .shannon/agents/*.toml (project-local)
  ~/.claude/agents/*.md (user-global)
  ~/.shannon/agents/*.toml (user-global)".to_string());
        }
        "list" => {
            let registry = AgentDefinitionRegistry::load_from_dirs();
            let loader = CustomAgentLoader::new();

            let custom_agents = match loader.discover() {
                Ok(agents) => agents,
                Err(e) => {
                    super::set_error(repl, &format!("loading custom agents: {e}"));
                    return Ok(());
                }
            };

            let mut output = String::new();

            let toml_defs = registry.list_names();
            if !toml_defs.is_empty() {
                output.push_str(&format!("TOML Agents ({}):\n", toml_defs.len()));
                for name in &toml_defs {
                    if let Some(def) = registry.get(name) {
                        let model = def.model.as_deref().unwrap_or("default");
                        let tools = if def.allowed_tools.is_empty() {
                            String::new()
                        } else {
                            format!(" tools=[{}]", def.allowed_tools.join(","))
                        };
                        output.push_str(&format!("  - {}{}: {} ({})\n", name, tools, def.description, model));
                    }
                }
            }

            let md_names: Vec<_> = custom_agents.keys().cloned().collect();
            if !md_names.is_empty() {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&format!("Markdown Agents ({}):\n", md_names.len()));
                for name in &md_names {
                    let def = &custom_agents[name];
                    let model = def.model.as_deref().unwrap_or("default");
                    let tools = def.allowed_tools.as_ref()
                        .map(|t| format!(" tools=[{}]", t.join(", ")))
                        .unwrap_or_default();
                    output.push_str(&format!("  - {}{}: {} ({})\n", name, tools, def.description, model));
                }
            }

            if output.is_empty() {
                output.push_str("No agent definitions found.\n");
                output.push_str("Create agents in .claude/agents/*.md or .shannon/agents/*.toml\n");
            }

            repl.chat.add_message(ChatRole::System, output);
        }
        "show" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agent show <name>".to_string());
                return Ok(());
            }

            let registry = AgentDefinitionRegistry::load_from_dirs();
            if let Some(def) = registry.get(name) {
                let mut output = format!("Agent: {} (TOML)\n", def.name);
                output.push_str(&format!("Description: {}\n", def.description));
                if let Some(model) = &def.model {
                    output.push_str(&format!("Model: {model}\n"));
                }
                if let Some(prompt) = &def.system_prompt {
                    output.push_str(&format!("System Prompt: {prompt}\n"));
                }
                if !def.allowed_tools.is_empty() {
                    output.push_str(&format!("Allowed Tools: {}\n", def.allowed_tools.join(", ")));
                }
                if !def.capabilities.is_empty() {
                    output.push_str(&format!("Capabilities: {}\n", def.capabilities.join(", ")));
                }
                output.push_str(&format!("Max Concurrent Tasks: {}\n", def.max_concurrent_tasks));
                if let Some(temp) = def.temperature {
                    output.push_str(&format!("Temperature: {temp}\n"));
                }
                repl.chat.add_message(ChatRole::System, output);
                return Ok(());
            }

            let loader = CustomAgentLoader::new();
            if let Ok(def) = loader.load(name) {
                let mut output = format!("Agent: {} (Markdown)\n", def.name);
                output.push_str(&format!("Description: {}\n", def.description));
                output.push_str(&format!("Source: {}\n", def.source_path.display()));
                if let Some(model) = &def.model {
                    output.push_str(&format!("Model: {model}\n"));
                }
                if let Some(tools) = &def.allowed_tools {
                    output.push_str(&format!("Allowed Tools: {}\n", tools.join(", ")));
                }
                if let Some(dirs) = &def.allowed_directories {
                    output.push_str(&format!("Allowed Directories: {}\n", dirs.join(", ")));
                }
                if let Some(max_turns) = def.max_turns {
                    output.push_str(&format!("Max Turns: {max_turns}\n"));
                }
                if !def.body_instructions.is_empty() {
                    output.push_str(&format!("Instructions:\n{}\n", def.body_instructions));
                }
                if let Some(suffix) = &def.system_prompt_suffix {
                    output.push_str(&format!("Prompt Suffix: {suffix}\n"));
                }
                repl.chat.add_message(ChatRole::System, output);
                return Ok(());
            }

            repl.chat.add_message(ChatRole::System, format!("Agent '{name}' not found."));
        }
        "run" => {
            let name = parts.get(1).copied().unwrap_or("");
            let prompt = parts.get(2).copied().unwrap_or("");

            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agent run <name> [prompt]".to_string());
                return Ok(());
            }

            let registry = AgentDefinitionRegistry::load_from_dirs();
            let config = if let Some(def) = registry.get(name) {
                let system_prompt = def.system_prompt.clone()
                    .unwrap_or_else(|| def.description.clone());
                Some((def.clone(), system_prompt))
            } else {
                let loader = CustomAgentLoader::new();
                if let Ok(def) = loader.load(name) {
                    let mut prompt_parts = Vec::new();
                    if !def.body_instructions.is_empty() {
                        prompt_parts.push(def.body_instructions.clone());
                    }
                    if let Some(suffix) = &def.system_prompt_suffix {
                        prompt_parts.push(suffix.clone());
                    }
                    let system_prompt = if prompt_parts.is_empty() {
                        def.description.clone()
                    } else {
                        prompt_parts.join("\n\n")
                    };

                    let toml_def = shannon_agents::agent_defs::AgentDefinition {
                        name: def.name.clone(),
                        description: def.description.clone(),
                        system_prompt: Some(system_prompt.clone()),
                        model: def.model.clone(),
                        capabilities: vec![],
                        allowed_tools: def.allowed_tools.unwrap_or_default(),
                        max_concurrent_tasks: 3,
                        plan_mode_required: false,
                        temperature: None,
                    };

                    Some((toml_def, system_prompt))
                } else {
                    None
                }
            };

            let (def, system_prompt) = match config {
                Some(c) => c,
                None => {
                    repl.chat.add_message(ChatRole::System, format!("Agent '{name}' not found. Use /agent list to see available agents."));
                    return Ok(());
                }
            };

            use shannon_agents::{AgentCoordinator, CoordinatorConfig, SubAgentRegistry, AgentConfig};

            if repl.agent_registry.is_none() {
                let config = CoordinatorConfig::default();
                let coordinator = match repl.runtime.block_on(AgentCoordinator::new(config)) {
                    Ok(c) => c,
                    Err(e) => {
                        super::set_error(repl, &format!("creating agent coordinator: {e}"));
                        return Ok(());
                    }
                };
                repl.agent_registry = Some(std::sync::Arc::new(SubAgentRegistry::new(
                    std::sync::Arc::new(coordinator),
                )));
            }

            let agent_config = AgentConfig {
                name: format!("agent-{}", def.name),
                model: def.model.clone().unwrap_or_else(|| {
                    repl.state.model.clone().unwrap_or_else(|| "claude-sonnet-4-6".to_string())
                }),
                system_prompt,
                tools: def.allowed_tools.clone(),
                working_directory: PathBuf::from("."),
                max_turns: def.max_concurrent_tasks as u32,
                team: None,
            };

            let registry = match repl.agent_registry.as_ref() {
                Some(r) => r.clone(),
                None => {
                    repl.chat.add_message(ChatRole::System, "Agent registry not available.".to_string());
                    return Ok(());
                }
            };
            match repl.runtime.block_on(registry.spawn(agent_config)) {
                Ok(agent) => {
                    repl.chat.add_message(ChatRole::System, format!(
                        "Agent '{}' spawned (id: {}, status: {})",
                        agent.name, agent.id, agent.status
                    ));

                    if !prompt.is_empty() {
                        match repl.runtime.block_on(registry.send_message("repl", &agent.name, serde_json::json!(prompt))) {
                            Ok(_) => {
                                repl.chat.add_message(ChatRole::System, format!("Message sent to agent '{}'.", agent.name));
                            }
                            Err(e) => {
                                super::set_error(repl, &format!("sending message to agent: {e}"));
                            }
                        }
                    }
                }
                Err(e) => {
                    super::set_error(repl, &format!("spawning agent: {e}"));
                }
            }
        }
        "create" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "\
Agent Creation Wizard
====================

Usage: /agent create <name>

This will guide you through creating an agent definition interactively.
The agent will be saved as a markdown file in .claude/agents/{name}.md".to_string());
                return Ok(());
            }

            if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                repl.chat.add_message(ChatRole::System, "Agent name must contain only alphanumeric characters, hyphens, and underscores.".to_string());
                return Ok(());
            }

            let registry = AgentDefinitionRegistry::load_from_dirs();
            if registry.get(name).is_some() {
                repl.chat.add_message(ChatRole::System, format!("Agent '{name}' already exists. Use /agent edit {name} to modify it."));
                return Ok(());
            }

            let loader = CustomAgentLoader::new();
            if loader.load(name).is_ok() {
                repl.chat.add_message(ChatRole::System, format!("Agent '{name}' already exists. Use /agent edit {name} to modify it."));
                return Ok(());
            }

            repl.state.pending_dialog_action = Some(format!("create_agent:{name}"));

            repl.chat.add_message(ChatRole::System, format!(
                "Creating agent '{name}'. Please provide the following information:\n\
                 1. Description: What does this agent do?\n\
                 2. Model (optional): opus, sonnet, or haiku (default: sonnet)\n\
                 3. Tools (optional): Comma-separated tool names\n\
                 4. Instructions: The agent's system prompt"
            ));
        }
        "edit" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agent edit <name>".to_string());
                return Ok(());
            }

            let registry = AgentDefinitionRegistry::load_from_dirs();
            let source_path = if let Some(_def) = registry.get(name) {
                repl.chat.add_message(ChatRole::System, format!(
                    "Agent '{name}' is defined in TOML format. Edit the file directly: .shannon/agents/{name}.toml"
                ));
                return Ok(());
            } else {
                let loader = CustomAgentLoader::new();
                match loader.load(name) {
                    Ok(def) => def.source_path.clone(),
                    Err(_) => {
                        repl.chat.add_message(ChatRole::System, format!("Agent '{name}' not found."));
                        return Ok(());
                    }
                }
            };

            repl.chat.add_message(ChatRole::System, format!(
                "Editing agent '{}' (source: {})\n\
                 To edit, modify the file directly and run /agent show {} to verify.",
                name, source_path.display(), name
            ));
        }
        _ => {
            repl.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /agent help."));
        }
    }

    Ok(())
}

pub(crate) fn handle_routine(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.trim().splitn(3, ' ').collect();
    let subcmd = parts.first().copied().unwrap_or("list");

    match subcmd {
        "list" | "ls" | "" => {
            let routines = repl.state.routine_manager.list();
            if routines.is_empty() {
                repl.chat.add_message(ChatRole::System, "No scheduled routines. Use /routine add <name> <interval_secs> <prompt>".to_string());
                return Ok(());
            }
            let mut msg = String::from("Scheduled Routines:\n\n");
            for r in routines {
                let status = if r.enabled { "ON" } else { "OFF" };
                let last = r.last_fired.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or("never".into());
                msg.push_str(&format!(
                    "  [{}] {} ({})\n    Interval: {}s | Fires: {} | Last: {}\n    Prompt: {}\n\n",
                    r.id, r.name, status, r.interval_secs, r.fire_count, last,
                    if r.prompt.len() > 60 { format!("{}...", &r.prompt[..57]) } else { r.prompt.clone() }
                ));
            }
            repl.chat.add_message(ChatRole::System, msg);
        }
        "add" => {
            if parts.len() < 4 {
                repl.chat.add_message(ChatRole::System,
                    "Usage: /routine add <name> <interval_secs> <prompt>\n\nExample: /routine add status-check 300 Check git status".to_string());
                return Ok(());
            }
            let name = parts[1].to_string();
            let interval: u64 = match parts[2].parse() {
                Ok(i) if i > 0 => i,
                _ => {
                    repl.chat.add_message(ChatRole::System, "Interval must be a positive number of seconds.".to_string());
                    return Ok(());
                }
            };
            let prompt = parts[3].to_string();
            let routine = shannon_core::scheduled_routines::ScheduledRoutine::new(name, prompt, interval);
            let id = routine.id.clone();
            repl.state.routine_manager.add(routine);
            repl.chat.add_message(ChatRole::System, format!("Added routine [{id}]. Use /routine list to see all."));
        }
        "remove" | "rm" | "delete" => {
            let id = parts.get(1).copied().unwrap_or("");
            if id.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /routine remove <id>".to_string());
                return Ok(());
            }
            match repl.state.routine_manager.remove(id) {
                Some(r) => repl.chat.add_message(ChatRole::System, format!("Removed routine: {}", r.name)),
                None => repl.chat.add_message(ChatRole::System, format!("Routine '{id}' not found.")),
            };
        }
        "toggle" => {
            let id = parts.get(1).copied().unwrap_or("");
            if id.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /routine toggle <id>".to_string());
                return Ok(());
            }
            match repl.state.routine_manager.toggle(id) {
                Some(enabled) => repl.chat.add_message(ChatRole::System,
                    format!("Routine {} is now {}", id, if enabled { "enabled" } else { "disabled" })),
                None => repl.chat.add_message(ChatRole::System, format!("Routine '{id}' not found.")),
            };
        }
        "fire" => {
            let due = repl.state.routine_manager.drain_due();
            if due.is_empty() {
                repl.chat.add_message(ChatRole::System, "No routines are due to fire.".to_string());
            } else {
                for (name, prompt) in due {
                    repl.chat.add_message(ChatRole::System, format!("Routine '{name}' fired: {prompt}"));
                }
            }
        }
        "save" => {
            let path = shannon_core::scheduled_routines::RoutineManager::default_storage_path();
            match repl.state.routine_manager.save_to_file(&path) {
                Ok(()) => { repl.chat.add_message(ChatRole::System, format!("Routines saved to {}", path.display())); }
                Err(e) => { super::set_error(repl, &format!("saving routines: {e}")); }
            };
        }
        "help" | "-h" | "--help" => {
            repl.chat.add_message(ChatRole::System,
                "Scheduled Routines — recurring task execution\n\n\
                 Commands:\n  /routine list                     — show all routines\n  \
                 /routine add <name> <secs> <prompt> — add a new routine\n  \
                 /routine remove <id>               — remove a routine\n  \
                 /routine toggle <id>               — enable/disable\n  \
                 /routine fire                      — manually check and fire due routines\n  \
                 /routine save                      — persist routines to disk".to_string());
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                format!("Unknown routine subcommand: '{subcmd}'. Use /routine help."));
        }
    }
    Ok(())
}
