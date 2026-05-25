//! Auto-loading project instructions from CLAUDE.md / AGENTS.md and cross-tool files.

use std::path::{Path, PathBuf};

/// Default filenames to search for, in priority order.
/// Includes cross-tool compatibility with Cursor, Windsurf, and Crush.
const INSTRUCTION_FILES: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    "GEMINI.md",
    ".cursorrules",
    ".windsurfrules",
    "CRUSH.md",
];

/// Default maximum recursion depth for @import resolution.
const DEFAULT_MAX_IMPORT_DEPTH: usize = 5;

/// Default maximum total imported content size in bytes.
const DEFAULT_MAX_IMPORT_SIZE: usize = 100 * 1024;

/// Instruction scope levels for hierarchical priority.
///
/// Priority order (highest to lowest):
/// 1. Managed - Remote/organization managed instructions
/// 2. Project - Project root instructions
/// 3. User - User-level instructions (~/.claude/)
/// 4. Local - Local project (.claude/, gitignored)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstructionScope {
    /// Managed/remote instructions (highest priority)
    Managed = 100,
    /// Project root instructions (repo root CLAUDE.md)
    Project = 75,
    /// Project root scope alias
    ProjectRoot = 74,
    /// User instructions (~/.claude/CLAUDE.md)
    User = 50,
    /// Global scope alias (user-level)
    Global = 49,
    /// Local project instructions (.claude/CLAUDE.md, gitignored)
    Local = 25,
    /// Directory-specific instructions (cwd-relative)
    Directory = 10,
}

impl InstructionScope {
    /// Get the display name for this scope.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Project | Self::ProjectRoot => "project",
            Self::User | Self::Global => "user",
            Self::Local => "local",
            Self::Directory => "directory",
        }
    }

    /// Get the display description for this scope.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Managed => "Managed/remote instructions (highest priority)",
            Self::Project | Self::ProjectRoot => "Project root instructions (repository root)",
            Self::User | Self::Global => "User instructions (~/.claude/)",
            Self::Local => "Local project (.claude/, .shannon/, gitignored)",
            Self::Directory => "Directory-specific instructions (cwd-relative)",
        }
    }
}

/// A single instruction file with its scope.
#[derive(Debug, Clone)]
pub struct InstructionFile {
    /// The file path.
    pub path: PathBuf,
    /// The content of the file.
    pub content: String,
    /// The scope level of this instruction.
    pub scope: InstructionScope,
}

/// Result of loading project instructions.
#[derive(Debug, Clone)]
pub struct ProjectInstructions {
    /// The combined content of all found instruction files.
    pub content: String,
    /// Which files were found and loaded.
    pub loaded_files: Vec<String>,
    /// Files pulled in via @import references.
    pub imported_files: Vec<String>,
    /// Individual instruction files with their scopes.
    pub instruction_files: Vec<InstructionFile>,
}

/// Load project instructions from the given directory and all parent directories.
///
/// Searches for `CLAUDE.md`, `AGENTS.md`, and `GEMINI.md` in the working directory
/// and each parent directory up to the filesystem root. Files found deeper in the
/// tree (closer to the working directory) are placed *after* those from parent
/// directories, so the most specific instructions come last and take visual precedence.
///
/// Returns `None` if no instruction files are found.
pub fn load_from_directory(dir: &Path) -> Option<ProjectInstructions> {
    load_from_directory_with_scope(dir, InstructionScope::Directory)
}

/// Load project instructions with a specific scope level.
///
/// This allows the caller to specify the scope level for the starting directory,
/// enabling proper hierarchical scoping.
fn load_from_directory_with_scope(
    dir: &Path,
    base_scope: InstructionScope,
) -> Option<ProjectInstructions> {
    let mut found: Vec<(PathBuf, String, InstructionScope)> = Vec::new();

    // Walk up from dir to root, collecting instruction files
    let mut current = Some(dir.to_path_buf());
    while let Some(path) = current.take() {
        // Determine scope for this level
        let scope = if path == dir {
            base_scope
        } else {
            // Parent directories get lower scope
            match base_scope {
                InstructionScope::Project => InstructionScope::User,
                InstructionScope::Local => InstructionScope::Project,
                _ => base_scope,
            }
        };

        for filename in INSTRUCTION_FILES {
            let candidate = path.join(filename);
            if candidate.is_file() {
                if let Ok(content) = std::fs::read_to_string(&candidate) {
                    if !content.trim().is_empty() {
                        found.push((candidate, content, scope));
                    }
                }
            }
        }
        current = path.parent().map(|p| p.to_path_buf());
    }

    if found.is_empty() {
        return None;
    }

    // Reverse so that root-level files come first, working-dir files last
    found.reverse();

    let loaded_files: Vec<String> = found
        .iter()
        .map(|(p, _, _)| {
            p.strip_prefix(dir)
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();

    let mut content = String::from("# Project Instructions\n\n");
    let mut all_imported_files: Vec<String> = Vec::new();
    let mut instruction_files: Vec<InstructionFile> = Vec::new();

    for (path, file_content, scope) in &found {
        let display_name = path.strip_prefix(dir).unwrap_or(path).to_string_lossy();
        let source_dir = path.parent().unwrap_or(dir);
        let (resolved, imported) = resolve_content_imports(
            file_content,
            source_dir,
            dir,
            DEFAULT_MAX_IMPORT_DEPTH,
            DEFAULT_MAX_IMPORT_SIZE,
        );
        all_imported_files.extend(imported);

        // Add scope header
        content.push_str(&format!(
            "## {} Scope: {} ---\n\n",
            scope.name(),
            display_name
        ));
        content.push_str(&resolved);
        content.push_str("\n\n");

        instruction_files.push(InstructionFile {
            path: path.clone(),
            content: file_content.clone(),
            scope: *scope,
        });
    }

    Some(ProjectInstructions {
        content,
        loaded_files,
        imported_files: all_imported_files,
        instruction_files,
    })
}

/// Load project instructions from the current working directory.
pub fn load_from_cwd() -> Option<ProjectInstructions> {
    std::env::current_dir()
        .ok()
        .and_then(|dir| load_from_directory(&dir))
}

/// Resolve `@import` references in content, relative to the source file.
///
/// An import reference is `@` followed by a path containing alphanumeric characters,
/// `/`, `.`, `-`, and `_`. References inside fenced code blocks (between ``` markers)
/// are left untouched.
///
/// Returns the resolved content and a list of imported file paths.
fn resolve_content_imports(
    content: &str,
    source_dir: &Path,
    project_root: &Path,
    max_depth: usize,
    max_total_size: usize,
) -> (String, Vec<String>) {
    let mut visited = std::collections::HashSet::<String>::new();
    let mut total_bytes = 0usize;
    let mut imported_files: Vec<String> = Vec::new();
    let resolved = resolve_content_imports_inner(
        content,
        source_dir,
        project_root,
        max_depth,
        max_total_size,
        &mut total_bytes,
        &mut visited,
        &mut imported_files,
    );
    (resolved, imported_files)
}

/// Inner recursive implementation that shares a visited set and byte counter.
#[allow(clippy::too_many_arguments)]
fn resolve_content_imports_inner(
    content: &str,
    source_dir: &Path,
    project_root: &Path,
    remaining_depth: usize,
    max_total_size: usize,
    total_imported_bytes: &mut usize,
    visited: &mut std::collections::HashSet<String>,
    imported_files: &mut Vec<String>,
) -> String {
    let mut resolved = String::with_capacity(content.len());
    let mut in_code_block = false;

    for line in content.lines() {
        // Track fenced code block boundaries
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            resolved.push_str(line);
            resolved.push('\n');
            continue;
        }

        if in_code_block {
            resolved.push_str(line);
            resolved.push('\n');
            continue;
        }

        // Process @import references in this line
        let processed = process_import_line_inner(
            line,
            source_dir,
            project_root,
            remaining_depth,
            max_total_size,
            total_imported_bytes,
            visited,
            imported_files,
        );
        resolved.push_str(&processed);
        resolved.push('\n');
    }

    // Remove trailing newline if the original didn't have one
    if !content.ends_with('\n') && resolved.ends_with('\n') {
        resolved.pop();
    }

    resolved
}

/// Process a single line, replacing @import references with file content.
#[allow(clippy::too_many_arguments)]
fn process_import_line_inner(
    line: &str,
    source_dir: &Path,
    project_root: &Path,
    remaining_depth: usize,
    max_total_size: usize,
    total_imported_bytes: &mut usize,
    visited: &mut std::collections::HashSet<String>,
    imported_files: &mut Vec<String>,
) -> String {
    let mut result = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            // Try to extract a valid import path after @
            if let Some(path_str) = extract_import_path(&chars[i + 1..]) {
                let full_path = source_dir.join(&path_str);

                // Security: reject path traversal outside the project root
                match full_path.canonicalize() {
                    Ok(canonical) => {
                        let root_canonical = match project_root.canonicalize() {
                            Ok(c) => c,
                            Err(_) => project_root.to_path_buf(),
                        };
                        if !canonical.starts_with(&root_canonical) {
                            eprintln!(
                                "warning: @import path '{path_str}' escapes project directory, skipping"
                            );
                            result.push('@');
                            i += 1;
                            continue;
                        }
                    }
                    Err(_) => {
                        // Path doesn't exist yet, will be caught by read below
                    }
                }

                // Check remaining depth
                if remaining_depth == 0 {
                    eprintln!("warning: @import max depth exceeded for '{path_str}', skipping");
                    result.push('@');
                    i += 1;
                    continue;
                }

                // Try to read and inline the file
                match std::fs::read_to_string(&full_path) {
                    Ok(file_content) => {
                        let new_bytes = file_content.len();
                        if *total_imported_bytes + new_bytes > max_total_size {
                            eprintln!(
                                "warning: @import total size limit exceeded at '{path_str}', skipping"
                            );
                            result.push('@');
                            i += 1;
                            continue;
                        }

                        // Detect circular import using canonical path via visited set
                        let visit_key = match full_path.canonicalize() {
                            Ok(c) => c.to_string_lossy().to_string(),
                            Err(_) => full_path.to_string_lossy().to_string(),
                        };
                        if visited.contains(&visit_key) {
                            eprintln!(
                                "warning: circular @import detected for '{path_str}', skipping"
                            );
                            result.push('@');
                            i += 1;
                            continue;
                        }

                        *total_imported_bytes += new_bytes;
                        visited.insert(visit_key);

                        let rel_path = full_path
                            .strip_prefix(project_root)
                            .unwrap_or(&full_path)
                            .to_string_lossy()
                            .to_string();
                        imported_files.push(rel_path);

                        // Recursively resolve imports in the imported file
                        let import_dir = full_path.parent().unwrap_or(source_dir);
                        let nested_content = resolve_content_imports_inner(
                            &file_content,
                            import_dir,
                            project_root,
                            remaining_depth - 1,
                            max_total_size,
                            total_imported_bytes,
                            visited,
                            imported_files,
                        );

                        result.push_str(&format!(
                            "--- {path_str} (imported) ---\n{nested_content}\n---\n"
                        ));

                        // Skip past the @path in the original line
                        i += 1 + path_str.len();
                        continue;
                    }
                    Err(e) => {
                        eprintln!("warning: @import file '{path_str}' not found: {e}");
                        result.push('@');
                        i += 1;
                        continue;
                    }
                }
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Extract a valid import path from the characters following `@`.
/// A valid path contains alphanumeric characters, `/`, `.`, `-`, and `_`.
/// Returns None if no valid path is found (e.g., `@` is followed by a space or special char).
fn extract_import_path(chars: &[char]) -> Option<String> {
    if chars.is_empty() {
        return None;
    }
    let first = chars[0];
    // First character must be alphanumeric, '.', '/', '_', or '-'
    if !first.is_alphanumeric() && first != '.' && first != '/' && first != '_' && first != '-' {
        return None;
    }

    let mut end = 0;
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphanumeric() || c == '/' || c == '.' || c == '-' || c == '_' {
            end = i + 1;
        } else {
            break;
        }
    }

    if end == 0 {
        return None;
    }

    let path: String = chars[..end].iter().collect();

    // Must have at least one non-extension character (reject bare "@.md")
    if path.starts_with('.') && !path.contains('/') {
        return None;
    }

    // Must contain a '.' to look like a file reference (e.g., "@RULES.md").
    // This rejects inline @mentions like "@agent-[name]" or "@agent-security".
    if !path.contains('.') {
        return None;
    }

    Some(path)
}

/// Gather git context (branch, recent commits, status summary) as a string.
/// Returns None if not in a git repo or git is unavailable.
pub fn git_context(dir: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let mut ctx = String::from("## Git Context\n\n");

    // Current branch
    if let Ok(branch_out) = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir)
        .output()
    {
        if branch_out.status.success() {
            let branch = String::from_utf8_lossy(&branch_out.stdout)
                .trim()
                .to_string();
            if !branch.is_empty() {
                ctx.push_str(&format!("Branch: {branch}\n"));
            }
        }
    }

    // Recent commits (last 5)
    if let Ok(log_out) = std::process::Command::new("git")
        .args(["log", "--oneline", "-5"])
        .current_dir(dir)
        .output()
    {
        if log_out.status.success() {
            let log = String::from_utf8_lossy(&log_out.stdout).trim().to_string();
            if !log.is_empty() {
                ctx.push_str(&format!("Recent commits:\n{log}\n"));
            }
        }
    }

    // Status summary
    if let Ok(status_out) = std::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(dir)
        .output()
    {
        if status_out.status.success() {
            let status = String::from_utf8_lossy(&status_out.stdout)
                .trim()
                .to_string();
            if !status.is_empty() {
                let count = status.lines().count();
                ctx.push_str(&format!("Working tree: {count} changed file(s)\n"));
            } else {
                ctx.push_str("Working tree: clean\n");
            }
        }
    }

    Some(ctx)
}

/// Load full project context: instruction files + git context.
/// Returns None only if nothing at all is available.
pub fn load_full_context(dir: &Path) -> Option<ProjectInstructions> {
    load_full_context_with_scopes(dir)
}

/// Load full project context with scope-aware organization.
///
/// This function loads instructions from multiple sources in priority order:
/// 1. Managed instructions (highest priority) - remote/organization settings
/// 2. Project instructions - repository root CLAUDE.md
/// 3. User instructions - ~/.claude/CLAUDE.md
/// 4. Local instructions - .claude/CLAUDE.md (gitignored)
/// 5. Git context - repository information
///
/// Priority: managed > project > user > local
///
/// Returns None only if nothing at all is available.
fn load_full_context_with_scopes(dir: &Path) -> Option<ProjectInstructions> {
    let mut all_content = String::new();
    let mut all_files: Vec<String> = Vec::new();
    let mut all_imported: Vec<String> = Vec::new();
    let mut instruction_files: Vec<InstructionFile> = Vec::new();

    // 1. Load managed instructions (highest priority)
    // Integrated with RemoteManagedSettings for organization-level instructions.
    // Gracefully skipped when no managed instructions are configured.
    if let Ok(Some(managed_content)) = load_managed_instructions(dir) {
        all_content.push_str(&format!(
            "## {} Scope: managed ---\n\n{}\n\n",
            InstructionScope::Managed.name(),
            managed_content
        ));
        all_files.push("managed instructions".to_string());
    }

    // 2. Load project-level instructions (walks up from dir to root, highest priority first)
    if let Some(proj) = load_from_directory_with_scope(dir, InstructionScope::Project) {
        // Skip the header since we already added it in load_from_directory_with_scope
        all_content.push_str(&proj.content.replacen("# Project Instructions\n\n", "", 1));
        all_files.extend(proj.loaded_files);
        all_imported.extend(proj.imported_files);
        instruction_files.extend(proj.instruction_files);
    }

    // 3. Load user-level global instructions from ~/.claude/CLAUDE.md
    if let Some(home) = dirs::home_dir() {
        let claude_dir = home.join(".claude");
        for filename in INSTRUCTION_FILES {
            let home_file = claude_dir.join(filename);
            if home_file.is_file() {
                if let Ok(content) = std::fs::read_to_string(&home_file) {
                    if !content.trim().is_empty() {
                        let (resolved, imported) = resolve_content_imports(
                            &content,
                            &claude_dir,
                            &claude_dir,
                            DEFAULT_MAX_IMPORT_DEPTH,
                            DEFAULT_MAX_IMPORT_SIZE,
                        );
                        all_imported.extend(imported);
                        all_content.push_str(&format!(
                            "## {} Scope: ~/.claude/{} ---\n\n{}\n\n",
                            InstructionScope::User.name(),
                            filename,
                            resolved
                        ));
                        all_files.push(format!("~/.claude/{filename}"));
                        instruction_files.push(InstructionFile {
                            path: home_file,
                            content,
                            scope: InstructionScope::User,
                        });
                    }
                }
            }
        }
    }

    // 4. Load local project instructions (gitignored, personal instructions)
    //    Checks: .claude/CLAUDE.md, .shannon/CLAUDE.md, CLAUDE.local.md
    //    (and equivalent AGENTS.md / GEMINI.md variants)
    let local_dirs: Vec<(&str, PathBuf)> = vec![
        (".claude", dir.join(".claude")),
        (".shannon", dir.join(".shannon")),
    ];
    let mut seen_local_paths = std::collections::HashSet::<PathBuf>::new();

    for (dir_name, local_dir) in &local_dirs {
        for filename in INSTRUCTION_FILES {
            let local_file = local_dir.join(filename);
            if local_file.is_file() {
                if let Ok(content) = std::fs::read_to_string(&local_file) {
                    if !content.trim().is_empty() {
                        let canonical = local_file
                            .canonicalize()
                            .unwrap_or_else(|_| local_file.clone());
                        if seen_local_paths.contains(&canonical) {
                            continue;
                        }
                        seen_local_paths.insert(canonical);

                        let (resolved, imported) = resolve_content_imports(
                            &content,
                            local_dir,
                            dir,
                            DEFAULT_MAX_IMPORT_DEPTH,
                            DEFAULT_MAX_IMPORT_SIZE,
                        );
                        all_imported.extend(imported);
                        let rel_path = format!("{dir_name}/{filename}");
                        all_content.push_str(&format!(
                            "## {} Scope: {} ---\n\n{}\n\n",
                            InstructionScope::Local.name(),
                            rel_path,
                            resolved
                        ));
                        all_files.push(rel_path);
                        instruction_files.push(InstructionFile {
                            path: local_file,
                            content,
                            scope: InstructionScope::Local,
                        });
                    }
                }
            }
        }
    }

    // Also load CLAUDE.local.md / AGENTS.local.md / GEMINI.local.md from project root
    for base_filename in INSTRUCTION_FILES {
        let local_filename = base_filename.replace(".md", ".local.md");
        let local_file = dir.join(&local_filename);
        if local_file.is_file() {
            if let Ok(content) = std::fs::read_to_string(&local_file) {
                if !content.trim().is_empty() {
                    let canonical = local_file
                        .canonicalize()
                        .unwrap_or_else(|_| local_file.clone());
                    if seen_local_paths.contains(&canonical) {
                        continue;
                    }
                    seen_local_paths.insert(canonical);

                    let (resolved, imported) = resolve_content_imports(
                        &content,
                        dir,
                        dir,
                        DEFAULT_MAX_IMPORT_DEPTH,
                        DEFAULT_MAX_IMPORT_SIZE,
                    );
                    all_imported.extend(imported);
                    all_content.push_str(&format!(
                        "## {} Scope: {} ---\n\n{}\n\n",
                        InstructionScope::Local.name(),
                        local_filename,
                        resolved
                    ));
                    all_files.push(local_filename.clone());
                    instruction_files.push(InstructionFile {
                        path: local_file,
                        content,
                        scope: InstructionScope::Local,
                    });
                }
            }
        }
    }

    // 5. Load git context
    if let Some(git) = git_context(dir) {
        all_content.push_str(&git);
        all_files.push("git context".to_string());
    }

    if all_content.is_empty() {
        None
    } else {
        Some(ProjectInstructions {
            content: all_content,
            loaded_files: all_files,
            imported_files: all_imported,
            instruction_files,
        })
    }
}

/// Load managed/organization instructions from `RemoteManagedSettings`.
///
/// Checks the remote managed settings store for a `"project_instructions"` key
/// (sourced from organization or remote overrides). If present and non-empty,
/// returns the content. Returns `Ok(None)` if no managed instructions are
/// configured or if the remote settings subsystem is unavailable.
fn load_managed_instructions(_dir: &Path) -> Result<Option<String>, Box<dyn std::error::Error>> {
    use crate::remote_settings::RemoteManagedSettings;

    let settings = RemoteManagedSettings::new();

    // The "project_instructions" key holds org/remote-level instruction content.
    if let Some(content) = settings.get("project_instructions") {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }

    Ok(None)
}

/// Get active instruction scopes for a directory.
///
/// Returns a list of scopes that have instruction files available.
pub fn get_active_scopes(dir: &Path) -> Vec<InstructionScope> {
    let mut scopes = Vec::new();

    // Check global scope
    if let Some(home) = dirs::home_dir() {
        let claude_dir = home.join(".claude");
        for filename in INSTRUCTION_FILES {
            if claude_dir.join(filename).is_file() {
                scopes.push(InstructionScope::Global);
                break;
            }
        }
    }

    // Check local scope (.claude/ and .shannon/)
    let mut found_local = false;
    for local_dir_name in &[".claude", ".shannon"] {
        let local_dir = dir.join(local_dir_name);
        for filename in INSTRUCTION_FILES {
            if local_dir.join(filename).is_file() {
                scopes.push(InstructionScope::Local);
                found_local = true;
                break;
            }
        }
        if found_local {
            break;
        }
    }

    // Check *.local.md files in project root
    if !found_local {
        for base_filename in INSTRUCTION_FILES {
            let local_filename = base_filename.replace(".md", ".local.md");
            if dir.join(&local_filename).is_file() {
                scopes.push(InstructionScope::Local);
                break;
            }
        }
    }

    // Check directory/project scope by walking up
    if load_from_directory_with_scope(dir, InstructionScope::Directory).is_some() {
        // Check if we're in a subdirectory (has parent instructions)
        let has_parent = dir.parent().is_some_and(|parent| {
            load_from_directory_with_scope(parent, InstructionScope::ProjectRoot).is_some()
        });
        scopes.push(if has_parent {
            InstructionScope::Directory
        } else {
            InstructionScope::ProjectRoot
        });
    }

    scopes.sort();
    scopes.dedup();
    scopes
}

/// Get detailed information about loaded instruction files.
///
/// Returns a formatted string showing which instruction files are loaded
/// at each scope level.
pub fn get_instruction_info(dir: &Path) -> String {
    let mut info = String::from("Instruction Files by Scope:\n\n");

    if let Some(instructions) = load_full_context_with_scopes(dir) {
        for file in &instructions.instruction_files {
            let display_path = if let Some(home) = dirs::home_dir() {
                file.path
                    .strip_prefix(&home)
                    .ok()
                    .and_then(|p| {
                        if p.starts_with(".claude") {
                            Some(format!("~/{}", p.to_string_lossy()))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        file.path
                            .strip_prefix(dir)
                            .ok()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| file.path.to_string_lossy().to_string())
                    })
            } else {
                file.path.to_string_lossy().to_string()
            };

            info.push_str(&format!(
                "  [{}] {} - {}\n",
                file.scope.name(),
                display_path,
                file.scope.description()
            ));
        }

        if instructions.instruction_files.is_empty() {
            info.push_str("  No instruction files found.\n");
        }
    } else {
        info.push_str("  No instruction context available.\n");
    }

    info
}

// ---------------------------------------------------------------------------
// InstructionWatcher — lightweight mtime-based hot-reload
// ---------------------------------------------------------------------------

/// Tracks modification times of project instruction files and detects changes.
///
/// Uses a simple mtime comparison — no external file-watching dependencies needed.
/// Call `check_and_reload()` before each query to detect changes and get updated
/// instructions.
#[derive(Debug)]
pub struct InstructionWatcher {
    /// The root directory to watch (project working directory).
    watch_dir: PathBuf,
    /// Map of file path → last known modification time.
    mtimes: std::collections::HashMap<PathBuf, std::time::SystemTime>,
    /// Cached combined instruction content (for when nothing changed).
    cached_content: Option<String>,
}

impl InstructionWatcher {
    /// Create a new watcher for the given working directory.
    pub fn new(watch_dir: PathBuf) -> Self {
        let mut watcher = Self {
            watch_dir,
            mtimes: std::collections::HashMap::new(),
            cached_content: None,
        };
        // Initial scan
        let _ = watcher.scan_mtimes();
        watcher
    }

    /// Scan all instruction files and record their mtimes.
    fn scan_mtimes(&mut self) -> std::collections::HashMap<PathBuf, std::time::SystemTime> {
        let mut current_mtimes = std::collections::HashMap::new();

        // Check home-level instructions (global scope)
        if let Some(home) = dirs::home_dir() {
            for filename in INSTRUCTION_FILES {
                let path = home.join(".claude").join(filename);
                if path.is_file() {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        if let Ok(mtime) = meta.modified() {
                            current_mtimes.insert(path, mtime);
                        }
                    }
                }
            }
        }

        // Check local project instructions (.claude/ and .shannon/ in current directory)
        for local_dir_name in &[".claude", ".shannon"] {
            let local_dir = self.watch_dir.join(local_dir_name);
            for filename in INSTRUCTION_FILES {
                let path = local_dir.join(filename);
                if path.is_file() {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        if let Ok(mtime) = meta.modified() {
                            current_mtimes.insert(path, mtime);
                        }
                    }
                }
            }
        }

        // Check *.local.md files in project root (CLAUDE.local.md, AGENTS.local.md, etc.)
        for base_filename in INSTRUCTION_FILES {
            let local_filename = base_filename.replace(".md", ".local.md");
            let path = self.watch_dir.join(&local_filename);
            if path.is_file() {
                if let Ok(meta) = std::fs::metadata(&path) {
                    if let Ok(mtime) = meta.modified() {
                        current_mtimes.insert(path, mtime);
                    }
                }
            }
        }

        // Walk up from watch_dir to root (project scope)
        let mut current = Some(self.watch_dir.clone());
        while let Some(path) = current.take() {
            for filename in INSTRUCTION_FILES {
                let candidate = path.join(filename);
                if candidate.is_file() {
                    if let Ok(meta) = std::fs::metadata(&candidate) {
                        if let Ok(mtime) = meta.modified() {
                            current_mtimes.insert(candidate, mtime);
                        }
                    }
                }
            }
            current = path.parent().map(|p| p.to_path_buf());
        }

        current_mtimes
    }

    /// Check if any instruction files have changed since the last check.
    ///
    /// Returns `Some((changed_files, new_content))` if files changed, `None` if unchanged.
    pub fn check_and_reload(&mut self) -> Option<(Vec<String>, String)> {
        let new_mtimes = self.scan_mtimes();

        // Check if mtimes changed or files added/removed
        let changed = new_mtimes.len() != self.mtimes.len()
            || new_mtimes
                .iter()
                .any(|(path, mtime)| self.mtimes.get(path) != Some(mtime));

        if !changed {
            return None;
        }

        // Reload instructions
        let changed_paths: Vec<String> = new_mtimes
            .keys()
            .filter(|p| {
                self.mtimes
                    .get(&**p)
                    .is_none_or(|old| new_mtimes.get(&**p).is_some_and(|cur| cur != old))
            })
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        self.mtimes = new_mtimes;

        let new_content = match load_full_context(&self.watch_dir) {
            Some(instr) => instr.content,
            None => String::new(),
        };

        self.cached_content = Some(new_content.clone());
        Some((changed_paths, new_content))
    }

    /// Get the cached instruction content (reload first if needed).
    pub fn cached_instructions(&self) -> Option<&str> {
        self.cached_content.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_nonexistent_dir() {
        assert!(load_from_directory(Path::new("/nonexistent/path/xyz")).is_none());
    }

    #[test]
    fn test_load_empty_dir() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        assert!(load_from_directory(&tmp).is_none());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_claude_md() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Test\n\nUse Rust best practices.").unwrap();

        let result = load_from_directory(&tmp);
        assert!(result.is_some());
        let instructions = result.unwrap();
        assert!(instructions.content.contains("Use Rust best practices"));
        assert!(instructions.loaded_files.contains(&"CLAUDE.md".to_string()));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_multiple_files() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Claude rules").unwrap();
        fs::write(tmp.join("AGENTS.md"), "# Agent rules").unwrap();

        let result = load_from_directory(&tmp);
        assert!(result.is_some());
        let instructions = result.unwrap();
        assert!(instructions.content.contains("Claude rules"));
        assert!(instructions.content.contains("Agent rules"));
        assert_eq!(instructions.loaded_files.len(), 2);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_empty_file_skipped() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "   \n  \n").unwrap();

        assert!(load_from_directory(&tmp).is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_parent_directory() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        let child = tmp.join("subdir");
        fs::create_dir_all(&child).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Parent project rules").unwrap();

        let result = load_from_directory(&child);
        assert!(result.is_some());
        assert!(result.unwrap().content.contains("Parent project rules"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_git_context_in_repo() {
        // This test runs in the shannon-code repo itself, so git context should work
        let cwd = std::env::current_dir().unwrap();
        let ctx = git_context(&cwd);
        // We're in a git repo, so should get Some
        assert!(ctx.is_some(), "Should get git context in a git repo");
        let ctx = ctx.unwrap();
        // Branch may be absent in CI (detached HEAD), but Recent commits should always be present
        if std::env::var("CI").is_ok() {
            assert!(
                ctx.contains("Recent commits") || ctx.contains("Git Context"),
                "Should contain git context info in CI"
            );
        } else {
            assert!(ctx.contains("Branch"), "Should contain branch info");
            assert!(
                ctx.contains("Recent commits"),
                "Should contain recent commits"
            );
        }
    }

    #[test]
    fn test_git_context_not_repo() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let ctx = git_context(&tmp);
        assert!(ctx.is_none(), "Should return None for non-git directory");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_full_context_with_git() {
        // Running in shannon-code repo: both instructions and git context should load
        let cwd = std::env::current_dir().unwrap();
        let result = load_full_context(&cwd);
        assert!(
            result.is_some(),
            "Should load full context in shannon-code repo"
        );
        let instr = result.unwrap();
        // Should have either CLAUDE.md or git context (or both)
        assert!(
            instr.loaded_files.contains(&"CLAUDE.md".to_string())
                || instr.loaded_files.contains(&"git context".to_string()),
            "Should load at least one source"
        );
    }

    #[test]
    fn test_load_full_context_nothing() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let result = load_full_context(&tmp);
        // May or may not be None depending on whether ~/.claude/CLAUDE.md exists
        // The important thing is it doesn't panic and returns a valid result
        if let Some(instr) = result {
            // If something was loaded, it should only be user-level or git context
            assert!(!instr.content.is_empty());
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_full_context_instructions_only() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Test instructions").unwrap();
        let result = load_full_context(&tmp);
        assert!(
            result.is_some(),
            "Should load instructions even without git"
        );
        let instr = result.unwrap();
        assert!(instr.content.contains("Test instructions"));
        assert!(instr.loaded_files.contains(&"CLAUDE.md".to_string()));
        let _ = fs::remove_dir_all(&tmp);
    }

    // -----------------------------------------------------------------------
    // @import resolution tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_import_simple() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("rules.md"), "Always use Rust best practices.").unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Project\n\n@rules.md\n").unwrap();

        let result = load_from_directory(&tmp).unwrap();
        assert!(
            result.content.contains("Always use Rust best practices"),
            "Imported content should appear: {:?}",
            result.content
        );
        assert!(
            result.content.contains("rules.md (imported)"),
            "Should have import header"
        );
        assert!(
            result.imported_files.iter().any(|f| f.contains("rules.md")),
            "rules.md should be in imported_files: {:?}",
            result.imported_files
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_import_nested() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("deep.md"), "Nested content here.").unwrap();
        fs::write(tmp.join("rules.md"), "Rules file.\n\n@deep.md\n").unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Project\n\n@rules.md\n").unwrap();

        let result = load_from_directory(&tmp).unwrap();
        assert!(
            result.content.contains("Nested content here"),
            "Nested import should resolve: {:?}",
            result.content
        );
        assert!(
            result.imported_files.iter().any(|f| f.contains("rules.md")),
            "rules.md in imported_files"
        );
        assert!(
            result.imported_files.iter().any(|f| f.contains("deep.md")),
            "deep.md in imported_files"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_import_circular_detection() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        // A imports B, B imports A -- should stop at circular detection
        fs::write(tmp.join("a.md"), "Content A.\n@b.md\n").unwrap();
        fs::write(tmp.join("b.md"), "Content B.\n@a.md\n").unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Project\n\n@a.md\n").unwrap();

        let result = load_from_directory(&tmp).unwrap();
        // Should contain both files' content but not loop infinitely
        assert!(
            result.content.contains("Content A"),
            "Should contain A: {:?}",
            result.content
        );
        assert!(
            result.content.contains("Content B"),
            "Should contain B: {:?}",
            result.content
        );
        // A should appear only once in imported_files
        let a_count = result
            .imported_files
            .iter()
            .filter(|f| f.contains("a.md"))
            .count();
        assert_eq!(
            a_count, 1,
            "a.md should appear exactly once in imported_files"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_import_missing_file() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(
            tmp.join("CLAUDE.md"),
            "# Project\n\n@nonexistent.md\nMore text.\n",
        )
        .unwrap();

        let result = load_from_directory(&tmp).unwrap();
        // Should not fail, just leave the @reference as-is
        assert!(
            result.content.contains("More text"),
            "Content after missing import should still appear: {:?}",
            result.content
        );
        assert!(
            result.imported_files.is_empty(),
            "No files should be imported: {:?}",
            result.imported_files
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_import_in_code_block_not_resolved() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("rules.md"), "Secret rules.").unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Project\n\n```\n@rules.md\n```\n").unwrap();

        let result = load_from_directory(&tmp).unwrap();
        // The @rules.md inside code block should NOT be resolved
        assert!(
            !result.content.contains("Secret rules"),
            "Code block import should NOT be resolved: {:?}",
            result.content
        );
        assert!(
            result.content.contains("@rules.md"),
            "Original @reference should remain in code block: {:?}",
            result.content
        );
        assert!(
            result.imported_files.is_empty(),
            "No imports from code blocks"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_import_size_limit() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        // Create a large file
        let big_content = "X".repeat(200);
        fs::write(tmp.join("big.md"), &big_content).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Project\n\n@big.md\n").unwrap();

        // Resolve with a very small size limit (50 bytes)
        let (resolved, imported) =
            resolve_content_imports("@big.md\n", &tmp, &tmp, DEFAULT_MAX_IMPORT_DEPTH, 50);
        // The file is too large to import under the 50-byte limit
        assert!(
            !resolved.contains("XXX"),
            "Large import should be skipped: {resolved:?}"
        );
        assert!(imported.is_empty(), "No files imported due to size limit");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_import_path_traversal_rejected() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        let subdir = tmp.join("project");
        fs::create_dir_all(&subdir).unwrap();
        // Write a file outside the project dir
        fs::write(tmp.join("secret.txt"), "secret data").unwrap();
        fs::write(subdir.join("CLAUDE.md"), "# Project\n\n@../secret.txt\n").unwrap();

        let result = load_from_directory(&subdir).unwrap();
        // Path traversal should be rejected
        assert!(
            !result.content.contains("secret data"),
            "Path traversal should be blocked: {:?}",
            result.content
        );
        assert!(
            result.imported_files.is_empty(),
            "No imports from path traversal"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_import_multiple_in_one_file() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("rules.md"), "Rule one.").unwrap();
        fs::write(tmp.join("flags.md"), "Flag settings.").unwrap();
        fs::write(
            tmp.join("CLAUDE.md"),
            "# Project\n\n@rules.md\n\nSome text.\n\n@flags.md\n",
        )
        .unwrap();

        let result = load_from_directory(&tmp).unwrap();
        assert!(
            result.content.contains("Rule one"),
            "First import: {:?}",
            result.content
        );
        assert!(
            result.content.contains("Flag settings"),
            "Second import: {:?}",
            result.content
        );
        assert!(
            result.content.contains("Some text"),
            "Text between imports preserved: {:?}",
            result.content
        );
        assert_eq!(
            result.imported_files.len(),
            2,
            "Should have 2 imported files: {:?}",
            result.imported_files
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_import_subdirectory_path() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(tmp.join("docs")).unwrap();
        fs::write(tmp.join("docs/guide.md"), "Guide content.").unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Project\n\n@docs/guide.md\n").unwrap();

        let result = load_from_directory(&tmp).unwrap();
        assert!(
            result.content.contains("Guide content"),
            "Subdirectory import should resolve: {:?}",
            result.content
        );
        assert!(
            result.imported_files.iter().any(|f| f.contains("guide.md")),
            "guide.md in imported_files"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_extract_import_path_valid() {
        assert_eq!(
            extract_import_path(&"rules.md".chars().collect::<Vec<_>>()),
            Some("rules.md".to_string())
        );
        assert_eq!(
            extract_import_path(&"docs/guide.md".chars().collect::<Vec<_>>()),
            Some("docs/guide.md".to_string())
        );
        assert_eq!(
            extract_import_path(&"my-file_v2.md".chars().collect::<Vec<_>>()),
            Some("my-file_v2.md".to_string())
        );
    }

    #[test]
    fn test_extract_import_path_invalid() {
        // Space after @
        assert_eq!(
            extract_import_path(&" rules.md".chars().collect::<Vec<_>>()),
            None
        );
        // Empty
        assert_eq!(extract_import_path(&[]), None);
        // Special chars
        assert_eq!(
            extract_import_path(&"@nested".chars().collect::<Vec<_>>()),
            None
        );
    }

    #[test]
    fn test_import_max_depth() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        // Create a chain: CLAUDE.md -> a.md -> b.md -> c.md -> d.md -> e.md -> f.md
        fs::write(tmp.join("f.md"), "Deepest content.").unwrap();
        fs::write(tmp.join("e.md"), "Level E.\n@f.md\n").unwrap();
        fs::write(tmp.join("d.md"), "Level D.\n@e.md\n").unwrap();
        fs::write(tmp.join("c.md"), "Level C.\n@d.md\n").unwrap();
        fs::write(tmp.join("b.md"), "Level B.\n@c.md\n").unwrap();
        fs::write(tmp.join("a.md"), "Level A.\n@b.md\n").unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Project\n\n@a.md\n").unwrap();

        // With max depth 5, should get several levels deep but not infinite loop
        let result = load_from_directory(&tmp).unwrap();
        assert!(result.content.contains("Level A"), "Should contain A");
        // The key is that we don't infinite loop and don't panic
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_instruction_cascade_order() {
        // Verify the instruction cascade priority: Managed > Project > User > Local
        // We test the scopes of loaded instruction_files to confirm the order.
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        // Write project-level CLAUDE.md
        fs::write(tmp.join("CLAUDE.md"), "# Project instructions").unwrap();

        // Write local .claude/CLAUDE.md
        fs::create_dir_all(tmp.join(".claude")).unwrap();
        fs::write(tmp.join(".claude/CLAUDE.md"), "# Local claude instructions").unwrap();

        // Write local .shannon/CLAUDE.md
        fs::create_dir_all(tmp.join(".shannon")).unwrap();
        fs::write(
            tmp.join(".shannon/CLAUDE.md"),
            "# Local shannon instructions",
        )
        .unwrap();

        // Write CLAUDE.local.md at project root
        fs::write(tmp.join("CLAUDE.local.md"), "# Local file instructions").unwrap();

        let result = load_full_context(&tmp);
        assert!(result.is_some(), "Should load full context");
        let instr = result.unwrap();

        // Collect scopes of all instruction files
        let scopes: Vec<InstructionScope> =
            instr.instruction_files.iter().map(|f| f.scope).collect();

        // Verify that Project scope exists (from CLAUDE.md at root)
        assert!(
            scopes.contains(&InstructionScope::Project),
            "Should have Project scope: {scopes:?}"
        );

        // Verify that Local scope exists (from .claude/, .shannon/, and CLAUDE.local.md)
        let local_count = scopes
            .iter()
            .filter(|s| **s == InstructionScope::Local)
            .count();
        assert!(
            local_count >= 1,
            "Should have at least one Local scope entry: {scopes:?}"
        );

        // Verify priority ordering: Managed > Project > User > Local
        // The instruction_files should have higher-priority scopes first
        let project_idx = instr
            .instruction_files
            .iter()
            .position(|f| f.scope == InstructionScope::Project);
        let local_idx = instr
            .instruction_files
            .iter()
            .position(|f| f.scope == InstructionScope::Local);

        if let (Some(pi), Some(li)) = (project_idx, local_idx) {
            assert!(
                pi < li,
                "Project scope (idx {pi}) should come before Local scope (idx {li})"
            );
        }

        // Verify scope value ordering (higher = higher priority)
        for window in instr.instruction_files.windows(2) {
            assert!(
                window[0].scope >= window[1].scope,
                "Scopes should be in descending priority order: {:?} >= {:?}",
                window[0].scope,
                window[1].scope
            );
        }

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_scope_priority_values() {
        // Verify the scope priority values match the expected ordering
        assert!(InstructionScope::Managed > InstructionScope::Project);
        assert!(InstructionScope::Project > InstructionScope::User);
        assert!(InstructionScope::User > InstructionScope::Local);
        assert!(InstructionScope::Local > InstructionScope::Directory);
    }

    #[test]
    fn test_shannon_dir_loaded_as_local() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(tmp.join(".shannon")).unwrap();
        fs::write(
            tmp.join(".shannon/CLAUDE.md"),
            "# Shannon local instructions",
        )
        .unwrap();

        let result = load_full_context(&tmp);
        assert!(result.is_some(), "Should load instructions from .shannon/");
        let instr = result.unwrap();

        // Content should include the .shannon instructions
        assert!(
            instr.content.contains("Shannon local instructions"),
            "Should contain .shannon content: {:?}",
            instr.content
        );

        // Should have at least one Local scope entry from .shannon/
        let has_shannon_local = instr
            .instruction_files
            .iter()
            .any(|f| f.scope == InstructionScope::Local && f.path.ends_with(".shannon/CLAUDE.md"));
        assert!(
            has_shannon_local,
            "Should have .shannon/CLAUDE.md as Local scope: {:?}",
            instr
                .instruction_files
                .iter()
                .map(|f| (&f.path, f.scope))
                .collect::<Vec<_>>()
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_claude_local_md_loaded_as_local() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(
            tmp.join("CLAUDE.local.md"),
            "# Personal gitignored instructions",
        )
        .unwrap();

        let result = load_full_context(&tmp);
        assert!(
            result.is_some(),
            "Should load instructions from CLAUDE.local.md"
        );
        let instr = result.unwrap();

        assert!(
            instr.content.contains("Personal gitignored instructions"),
            "Should contain CLAUDE.local.md content: {:?}",
            instr.content
        );

        let has_local = instr
            .instruction_files
            .iter()
            .any(|f| f.scope == InstructionScope::Local && f.path.ends_with("CLAUDE.local.md"));
        assert!(
            has_local,
            "Should have CLAUDE.local.md as Local scope: {:?}",
            instr
                .instruction_files
                .iter()
                .map(|f| (&f.path, f.scope))
                .collect::<Vec<_>>()
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_managed_instructions_graceful_skip() {
        // When no RemoteManagedSettings are configured, managed instructions should be skipped gracefully
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Only project instructions").unwrap();

        let result = load_full_context(&tmp);
        assert!(result.is_some());
        let instr = result.unwrap();

        // Should NOT contain managed scope header (since no managed instructions configured)
        assert!(
            !instr.content.contains("managed Scope"),
            "Should not have managed scope when no remote settings are configured: {:?}",
            instr.content
        );

        // But project instructions should still load fine
        assert!(
            instr.content.contains("Only project instructions"),
            "Should still load project instructions: {:?}",
            instr.content
        );

        let _ = fs::remove_dir_all(&tmp);
    }
}
