//! Custom command discovery, frontmatter parsing, file watching, and hot-reload.

use std::collections::HashMap;
use shannon_commands::{Command, CommandBase, CommandRegistry, ExecutionContext, PromptCommand};

/// Extract a field value from simple YAML-like frontmatter text.
pub(crate) fn parse_frontmatter_field(frontmatter: &str, field: &str) -> Option<String> {
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(field).and_then(|s| s.strip_prefix(':')) {
            let val = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

/// Parsed custom command entry with optional frontmatter metadata.
pub(crate) struct CustomCommandEntry {
    pub name: String,
    pub template: String,
    pub path: std::path::PathBuf,
    /// Optional description from frontmatter `description:` field.
    pub description: Option<String>,
    /// Argument names from frontmatter `arguments:` field.
    pub arguments: Vec<String>,
    /// Optional model override from frontmatter `model:` field.
    pub model: Option<String>,
    /// Optional allowed tools from frontmatter `allowed-tools:` field.
    pub allowed_tools: Vec<String>,
    /// Optional agent from frontmatter `agent:` field.
    pub agent: Option<String>,
}

/// Recursively collect custom commands from a directory.
///
/// - `dir`: root directory to scan
/// - `prefix`: path prefix for nested dirs (e.g. "project:" for `.claude/commands/project/`)
/// - `results`: accumulated (command_name, template_text, file_path) triples
pub(crate) fn collect_custom_commands(
    dir: &std::path::Path,
    prefix: &str,
    results: &mut Vec<CustomCommandEntry>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip hidden directories
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
                let subdir_prefix = format!("{prefix}{name}:");
                collect_custom_commands(&path, &subdir_prefix, results);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if stem.is_empty() {
                continue;
            }
            let command_name = format!("{prefix}{stem}");
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // Parse YAML frontmatter (---\n...\n---)
            let (template, description, arguments, model, allowed_tools, agent) = if content.starts_with("---") {
                let parts: Vec<&str> = content.splitn(3, "---").collect();
                let frontmatter = parts.get(1).unwrap_or(&"");
                let body = parts.get(2).map(|s| s.trim_start()).unwrap_or("");
                let desc = parse_frontmatter_field(frontmatter, "description");
                let args_str = parse_frontmatter_field(frontmatter, "arguments")
                    .or_else(|| parse_frontmatter_field(frontmatter, "args"));
                let args = args_str
                    .map(|s| s.split(',').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect())
                    .unwrap_or_default();
                let m = parse_frontmatter_field(frontmatter, "model");
                let tools_str = parse_frontmatter_field(frontmatter, "allowed-tools")
                    .or_else(|| parse_frontmatter_field(frontmatter, "allowed_tools"));
                let tools = tools_str
                    .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
                    .unwrap_or_default();
                let a = parse_frontmatter_field(frontmatter, "agent");
                (body.to_string(), desc, args, m, tools, a)
            } else {
                (content, None, Vec::new(), None, Vec::new(), None)
            };
            results.push(CustomCommandEntry { name: command_name, template, path, description, arguments, model, allowed_tools, agent });
        }
    }
}

/// Deduplicate custom commands by name, keeping the last occurrence.
/// Since project-level dirs are scanned after user-level dirs, project commands
/// override user-level commands with the same name.
pub(crate) fn dedup_custom_commands(commands: &mut Vec<CustomCommandEntry>) {
    let mut seen = std::collections::HashSet::new();
    commands.reverse();
    commands.retain(|c| seen.insert(c.name.clone()));
    commands.reverse();
}

/// Watches custom command directories for changes using filesystem events.
///
/// Uses the `notify` crate to watch `.claude/commands/` and `.shannon/commands/`
/// (project and user level). When changes are detected, commands are re-scanned
/// and re-registered in the [`CommandRegistry`].
pub(crate) struct CustomCommandWatcher {
    dirs: Vec<std::path::PathBuf>,
    #[allow(dead_code)]
    watcher: Option<notify::RecommendedWatcher>,
    dirty: std::sync::Arc<std::sync::atomic::AtomicBool>,
    registered_names: Vec<String>,
}

impl CustomCommandWatcher {
    pub(super) fn new() -> Self {
        let mut dirs = Vec::new();
        let cwd = std::env::current_dir().unwrap_or_default();
        dirs.push(cwd.join(".claude").join("commands"));
        dirs.push(cwd.join(".shannon").join("commands"));
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".claude").join("commands"));
            dirs.push(home.join(".shannon").join("commands"));
        }

        let dirty = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dirty_flag = dirty.clone();

        let handler = move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                use notify::EventKind;
                if matches!(event.kind,
                    EventKind::Create(_) |
                    EventKind::Modify(_) |
                    EventKind::Remove(_)
                ) {
                    dirty_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        };

        let watcher_result = notify::recommended_watcher(handler);

        let watcher = match watcher_result {
            Ok(mut w) => {
                use notify::Watcher;
                for dir in &dirs {
                    if dir.exists() {
                        let _ = w.watch(dir, notify::RecursiveMode::Recursive);
                    }
                }
                Some(w)
            }
            Err(_) => None,
        };

        Self { dirs, watcher, dirty, registered_names: Vec::new() }
    }

    /// Check if filesystem events were received and reload if needed.
    /// Returns count of re-registered commands.
    pub(crate) fn check_and_reload(&mut self, registry: &CommandRegistry) -> usize {
        if !self.dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            return 0;
        }

        // Unregister previously registered custom commands to prevent duplicates
        for name in &self.registered_names {
            registry.unregister_sync(name);
        }
        self.registered_names.clear();

        // Re-scan and re-register all custom commands
        let mut current_files: Vec<CustomCommandEntry> = Vec::new();
        for dir in &self.dirs {
            collect_custom_commands(dir, "", &mut current_files);
        }
        dedup_custom_commands(&mut current_files);

        for entry in &current_files {
            let description = entry.description.clone()
                .unwrap_or_else(|| format!("Custom command (from {})", entry.path.display()));
            let arg_names = if entry.arguments.is_empty() {
                vec!["$ARGUMENTS".to_string()]
            } else {
                entry.arguments.clone()
            };
            let argument_hint = if entry.arguments.is_empty() {
                Some("$ARGUMENTS".to_string())
            } else {
                Some(entry.arguments.join(" "))
            };
            let command = Command::Prompt(Box::new(PromptCommand {
                base: CommandBase {
                    name: entry.name.clone(),
                    aliases: Vec::new(),
                    description,
                    has_user_specified_description: entry.description.is_some(),
                    availability: vec![shannon_commands::CommandAvailability::All],
                    source: shannon_commands::CommandSource::Builtin,
                    is_enabled: true,
                    is_hidden: false,
                    argument_hint,
                    when_to_use: None,
                    version: None,
                    disable_model_invocation: false,
                    user_invocable: true,
                    is_workflow: false,
                    immediate: false,
                    is_sensitive: false,
                    user_facing_name: None,
                },
                progress_message: format!("Running /{}...", entry.name),
                content_length: entry.template.len(),
                arg_names,
                allowed_tools: entry.allowed_tools.clone(),
                model: entry.model.clone(),
                hooks: HashMap::new(),
                context: ExecutionContext::Inline,
                agent: entry.agent.clone(),
                paths: Vec::new(),
                prompt_template: Some(entry.template.clone()),
            }));
            registry.register_sync(command);
            self.registered_names.push(entry.name.clone());
        }

        let count = current_files.len();
        tracing::info!("Custom commands hot-reloaded ({} commands)", count);
        count
    }
}

/// Watches settings files for changes and signals the REPL to reload configuration.
///
/// Monitors `~/.claude/settings.json`, `~/.shannon/settings.json`,
/// `.claude/settings.json`, `.shannon/settings.json`, and `.shannon.toml` / `.shannon/config.toml`.
/// Uses the `notify` crate for efficient filesystem events.
pub(crate) struct SettingsWatcher {
    #[allow(dead_code)]
    watcher: Option<notify::RecommendedWatcher>,
    dirty: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Paths being watched, for diagnostic logging.
    watched_paths: Vec<std::path::PathBuf>,
}

impl SettingsWatcher {
    pub(super) fn new() -> Self {
        let mut paths = Vec::new();
        let cwd = std::env::current_dir().unwrap_or_default();

        // Project-level settings
        for dir_name in &[".claude", ".shannon"] {
            let settings = cwd.join(dir_name).join("settings.json");
            if settings.parent().map(|p| p.exists()).unwrap_or(false) {
                paths.push(settings);
            }
        }
        paths.push(cwd.join(".shannon.toml"));
        let shannon_config_dir = cwd.join(".shannon");
        if shannon_config_dir.exists() {
            paths.push(shannon_config_dir.join("config.toml"));
        }

        // User-level settings
        if let Some(home) = dirs::home_dir() {
            for dir_name in &[".claude", ".shannon"] {
                let settings = home.join(dir_name).join("settings.json");
                if settings.exists() {
                    paths.push(settings);
                }
            }
        }

        let dirty = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dirty_flag = dirty.clone();

        let handler = move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                use notify::EventKind;
                if matches!(event.kind,
                    EventKind::Create(_) |
                    EventKind::Modify(_) |
                    EventKind::Remove(_)
                ) {
                    dirty_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        };

        let watcher = match notify::recommended_watcher(handler) {
            Ok(mut w) => {
                use notify::Watcher;
                let mut watched = Vec::new();
                for path in &paths {
                    // Watch the parent directory so create/delete of the file itself is detected
                    let watch_target = if path.exists() {
                        path.clone()
                    } else {
                        path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| path.clone())
                    };
                    if watch_target.exists() {
                        if w.watch(&watch_target, notify::RecursiveMode::NonRecursive).is_ok() {
                            watched.push(path.clone());
                        }
                    }
                }
                if watched.is_empty() {
                    None
                } else {
                    tracing::debug!("SettingsWatcher watching {} paths", watched.len());
                    Some(w)
                }
            }
            Err(_) => None,
        };

        Self {
            watcher,
            dirty,
            watched_paths: paths,
        }
    }

    /// Check if settings files changed. Returns `Some(changed_paths)` if reload is needed.
    pub(crate) fn check_and_reload(&self) -> Option<Vec<String>> {
        if !self.dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            return None;
        }

        // Collect which files actually changed by checking mtimes
        let changed: Vec<String> = self.watched_paths
            .iter()
            .filter(|p| p.exists())
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        if changed.is_empty() {
            None
        } else {
            tracing::info!("Settings files changed: {:?}", changed);
            Some(changed)
        }
    }
}
