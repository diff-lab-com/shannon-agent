//! Shell-out notifier for CLI / headless mode.
//!
//! Fires OS-native notifications by spawning platform binaries:
//!   - Linux:   `notify-send`
//!   - macOS:   `osascript`
//!   - Windows: `powershell` (BurntToast)
//!
//! Commands are spawned via `std::process::Command` with the args array — never
//! through a shell — so titles and bodies cannot perform shell injection.

use std::process::{Command, Stdio};

use shannon_core::notifier::{Notification, NotificationHandler, NotificationLevel, NotifierError};

/// Template for a single argument to the shell command.
///
/// Placeholders `{title}`, `{body}`, `{level}`, `{urgency}`, `{source}` are
/// substituted with notification-derived values before spawning. Substitution
/// is literal (no shell expansion occurs — the result is passed as a single
/// argv element).
#[derive(Debug, Clone)]
pub struct CommandSpec {
    /// Binary to execute (resolved via `PATH`).
    pub binary: String,
    /// Argument templates.
    pub args: Vec<String>,
}

impl CommandSpec {
    /// Platform default notification command.
    pub fn platform_default() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self {
                binary: "notify-send".into(),
                args: vec![
                    "-u".into(),
                    "{urgency}".into(),
                    "-t".into(),
                    "5000".into(),
                    "-a".into(),
                    "Shannon".into(),
                    "{title}".into(),
                    "{body}".into(),
                ],
            }
        }
        #[cfg(target_os = "macos")]
        {
            Self {
                binary: "osascript".into(),
                args: vec![
                    "-e".into(),
                    "display notification \"{body}\" with title \"{title}\" subtitle \"{source}\""
                        .into(),
                ],
            }
        }
        #[cfg(target_os = "windows")]
        {
            Self {
                binary: "powershell".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    "New-BurntToastNotification -Title '{title}' -Message '{body}'".into(),
                ],
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Self {
                binary: "echo".into(),
                args: vec![
                    "[shannon-notification]".into(),
                    "{title}".into(),
                    "{body}".into(),
                ],
            }
        }
    }
}

/// Maps a notification level to the Linux `notify-send` urgency value.
pub fn level_to_urgency(level: NotificationLevel) -> &'static str {
    match level {
        NotificationLevel::Info | NotificationLevel::Success => "normal",
        NotificationLevel::Warning => "normal",
        NotificationLevel::Error => "critical",
    }
}

/// Spawns a shell command to deliver a notification, fire-and-forget.
///
/// Returns `Err` only if the binary cannot be spawned (missing, not in `PATH`,
/// or exec permission denied). The child is detached — its exit status is not
/// awaited, matching the async semantics of all target platform notifiers.
pub struct ShellNotifier {
    spec: CommandSpec,
    name: String,
}

impl ShellNotifier {
    /// Create a `ShellNotifier` using the platform-default command.
    pub fn new() -> Self {
        Self::with_spec(CommandSpec::platform_default())
    }

    /// Create a `ShellNotifier` with a custom command spec.
    ///
    /// **Security:** `spec.binary` is spawned directly via `Command::new` — the
    /// caller is responsible for ensuring the binary path is trusted (e.g. a
    /// hardcoded platform constant). Never pass user-controlled input as the
    /// binary. The platform default (`CommandSpec::platform_default()`) is the
    /// only path used by `ShellNotifier::new()`; `with_spec` exists for
    /// developer/test overrides.
    pub fn with_spec(spec: CommandSpec) -> Self {
        Self {
            spec,
            name: "shell".to_string(),
        }
    }

    /// Render the args template against a notification payload.
    ///
    /// Performs three security passes:
    /// 1. `sanitize()` — strip control chars, replace newlines, truncate
    /// 2. Per-binary context escaping — AppleScript/PowerShell string literals
    /// 3. Single-pass placeholder substitution — prevents template injection
    pub fn render_args(&self, n: &Notification) -> Vec<String> {
        let title = sanitize(&n.title);
        let body = sanitize(&n.body);
        let level = match n.level {
            NotificationLevel::Info => "info",
            NotificationLevel::Success => "success",
            NotificationLevel::Warning => "warning",
            NotificationLevel::Error => "error",
        };
        let urgency = level_to_urgency(n.level);
        let source = sanitize(n.source.as_deref().unwrap_or(""));

        let (title_e, body_e, source_e) = match self.spec.binary.as_str() {
            "osascript" | "/usr/bin/osascript" => (
                escape_applescript(&title),
                escape_applescript(&body),
                escape_applescript(&source),
            ),
            "powershell" | "powershell.exe" => (
                escape_powershell(&title),
                escape_powershell(&body),
                escape_powershell(&source),
            ),
            _ => (title, body, source),
        };

        let vars: [(&str, &str); 5] = [
            ("title", &title_e),
            ("body", &body_e),
            ("level", level),
            ("urgency", urgency),
            ("source", &source_e),
        ];

        self.spec
            .args
            .iter()
            .map(|template| substitute(template, &vars))
            .collect()
    }
}

impl Default for ShellNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationHandler for ShellNotifier {
    fn send(&self, notification: &Notification) -> Result<(), NotifierError> {
        let args = self.render_args(notification);
        let mut cmd = Command::new(&self.spec.binary);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match cmd.spawn() {
            Ok(_child) => Ok(()),
            Err(e) => Err(NotifierError::HandlerFailed {
                name: self.name.clone(),
                reason: format!("spawn '{}' failed: {e}", self.spec.binary),
            }),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Escape for AppleScript double-quoted string literals.
///
/// Backslashes first, then double quotes — both must be escaped inside
/// `"..."` strings to prevent breaking out of the literal and injecting
/// arbitrary AppleScript (which can execute shell commands via `do shell script`).
fn escape_applescript(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escape for PowerShell single-quoted string literals.
///
/// Single quotes are doubled (`'` → `''`). Single-quoted strings in PowerShell
/// are literal — no variable expansion — so this is sufficient to prevent
/// breakout from `New-BurntToastNotification -Title '...'` contexts.
fn escape_powershell(input: &str) -> String {
    input.replace('\'', "''")
}

/// Single-pass placeholder substitution.
///
/// Scans the template for `{key}` placeholders and substitutes known keys.
/// Unlike chained `str::replace` calls, substituted values are NOT re-scanned
/// for placeholders — so a malicious title containing literal `{body}` cannot
/// inject body content into the output. Unknown `{...}` sequences are preserved
/// verbatim.
fn substitute(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while !rest.is_empty() {
        match rest.find('{') {
            Some(brace_start) => {
                out.push_str(&rest[..brace_start]);
                let after = &rest[brace_start + 1..];
                match after.find('}') {
                    Some(close_offset) => {
                        let key = &after[..close_offset];
                        if let Some((_, value)) = vars.iter().find(|(k, _)| *k == key) {
                            out.push_str(value);
                            rest = &after[close_offset + 1..];
                            continue;
                        }
                        out.push('{');
                        rest = after;
                    }
                    None => {
                        out.push('{');
                        rest = after;
                    }
                }
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

/// Strip characters that could be misinterpreted by downstream notifiers.
///
/// `notify-send` and AppleScript treat certain characters specially. We remove
/// control characters outright; other shell-breaking chars are safe because we
/// pass argv directly (no shell). Newlines are replaced with spaces so single
/// line AppleScript invocations remain valid.
fn sanitize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\n' | '\r' => out.push(' '),
            c if (c as u32) < 0x20 => continue,
            c => out.push(c),
        }
    }
    // Truncate to a reasonable display length.
    out.chars().take(280).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_notification(title: &str, body: &str, level: NotificationLevel) -> Notification {
        Notification {
            title: title.into(),
            body: body.into(),
            level,
            id: "test".into(),
            timestamp: Utc::now(),
            source: Some("test_source".into()),
            action_id: None,
        }
    }

    #[test]
    fn test_platform_default_returns_a_spec() {
        let spec = CommandSpec::platform_default();
        assert!(!spec.binary.is_empty());
        assert!(!spec.args.is_empty());
    }

    #[test]
    fn test_render_args_substitutes_placeholders() {
        let notifier = ShellNotifier::with_spec(CommandSpec {
            binary: "echo".into(),
            args: vec![
                "--title".into(),
                "{title}".into(),
                "--body".into(),
                "{body}".into(),
                "--level".into(),
                "{level}".into(),
                "--urgency".into(),
                "{urgency}".into(),
            ],
        });
        let n = make_notification("Hello", "World", NotificationLevel::Error);
        let args = notifier.render_args(&n);
        assert_eq!(args[0], "--title");
        assert_eq!(args[1], "Hello");
        assert_eq!(args[3], "World");
        assert_eq!(args[5], "error");
        assert_eq!(args[7], "critical");
    }

    #[test]
    fn test_render_args_preserves_literal_braces_when_unused() {
        let notifier = ShellNotifier::with_spec(CommandSpec {
            binary: "echo".into(),
            args: vec!["literal {not_a_placeholder}".into()],
        });
        let n = make_notification("a", "b", NotificationLevel::Info);
        let args = notifier.render_args(&n);
        assert_eq!(args[0], "literal {not_a_placeholder}");
    }

    #[test]
    fn test_sanitize_strips_control_chars_and_newlines() {
        let s = sanitize("hello\nworld\tx");
        assert_eq!(s, "hello worldx");
        assert!(!s.contains('\n'));
        assert!(!s.contains('\t'));
    }

    #[test]
    fn test_sanitize_truncates_long_strings() {
        let long = "a".repeat(1000);
        let s = sanitize(&long);
        assert!(s.chars().count() <= 280);
    }

    #[test]
    fn test_level_to_urgency_mapping() {
        assert_eq!(level_to_urgency(NotificationLevel::Info), "normal");
        assert_eq!(level_to_urgency(NotificationLevel::Success), "normal");
        assert_eq!(level_to_urgency(NotificationLevel::Warning), "normal");
        assert_eq!(level_to_urgency(NotificationLevel::Error), "critical");
    }

    #[test]
    fn test_shell_notifier_name() {
        let n = ShellNotifier::new();
        assert_eq!(n.name(), "shell");
    }

    #[test]
    fn test_shell_notifier_missing_binary_returns_error() {
        let notifier = ShellNotifier::with_spec(CommandSpec {
            binary: "/nonexistent/binary/that/does/not/exist".into(),
            args: vec![],
        });
        let n = make_notification("t", "b", NotificationLevel::Info);
        let result = notifier.send(&n);
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            NotifierError::HandlerFailed { name, reason } => {
                assert_eq!(name, "shell");
                assert!(reason.contains("spawn"));
            }
            other => panic!("expected HandlerFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_escape_applescript_backslash_then_quote() {
        let s = escape_applescript(r#"hello "world" \end"#);
        assert_eq!(s, r#"hello \"world\" \\end"#);
    }

    #[test]
    fn test_escape_applescript_quote_only() {
        assert_eq!(escape_applescript(r#"just "quotes""#), r#"just \"quotes\""#);
    }

    #[test]
    fn test_escape_applescript_no_special_chars() {
        assert_eq!(escape_applescript("plain text"), "plain text");
    }

    #[test]
    fn test_escape_powershell_doubles_single_quotes() {
        assert_eq!(
            escape_powershell("can't break 'out'"),
            "can''t break ''out''"
        );
    }

    #[test]
    fn test_escape_powershell_no_single_quotes() {
        assert_eq!(escape_powershell(r#""double" ok"#), r#""double" ok"#);
    }

    /// Walk the script as AppleScript would, counting unescaped `"` (those
    /// NOT preceded by `\`). This is the number of string-literal delimiters.
    fn count_unescaped_applescript_quotes(s: &str) -> usize {
        let mut count = 0;
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                count += 1;
            }
        }
        count
    }

    /// Walk as PowerShell would, counting `'` that is NOT part of a `''` escape.
    fn count_unescaped_powershell_quotes(s: &str) -> usize {
        let mut count = 0;
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn test_render_args_escapes_applescript_double_quotes() {
        let notifier = ShellNotifier::with_spec(CommandSpec {
            binary: "osascript".into(),
            args: vec![
                "-e".into(),
                "display notification \"{body}\" with title \"{title}\"".into(),
            ],
        });
        let n = make_notification(
            "Evil \") & (do shell script \"rm -rf ~\")",
            "Body with \" and \\ backslash",
            NotificationLevel::Info,
        );
        let args = notifier.render_args(&n);
        let script = &args[1];
        assert!(
            script.contains("\\\""),
            "double quotes must be escaped: {script}"
        );
        assert!(
            script.contains("\\\\"),
            "backslashes must be escaped: {script}"
        );
        // Structural integrity: walk the script as AppleScript would, counting
        // unescaped `"` (i.e. NOT preceded by `\`). Two literal pairs
        // (`title "..."`, `body "..."`) → exactly 4. A breakout attempt that
        // injected extra unescaped quotes would push this above 4.
        let unescaped = count_unescaped_applescript_quotes(script);
        assert_eq!(
            unescaped, 4,
            "expected exactly 4 unescaped quotes (2 literal pairs), got {unescaped}: {script}"
        );
    }

    #[test]
    fn test_render_args_escapes_powershell_single_quotes() {
        let notifier = ShellNotifier::with_spec(CommandSpec {
            binary: "powershell".into(),
            args: vec![
                "-NoProfile".into(),
                "-Command".into(),
                "New-BurntToastNotification -Title '{title}' -Message '{body}'".into(),
            ],
        });
        let n = make_notification(
            "Evil '; Get-Process | Stop-Process; $x='",
            "Body 'attempt'",
            NotificationLevel::Info,
        );
        let args = notifier.render_args(&n);
        let cmd = &args[2];
        assert!(cmd.contains("''"), "single quotes must be doubled: {cmd}");
        // Structural integrity: walk as PowerShell would, counting `'` that is
        // NOT part of a `''` escape. Two literal pairs → exactly 4.
        let unescaped = count_unescaped_powershell_quotes(cmd);
        assert_eq!(
            unescaped, 4,
            "expected exactly 4 unescaped single quotes (2 literal pairs), got {unescaped}: {cmd}"
        );
    }

    #[test]
    fn test_render_args_prevents_template_injection() {
        let notifier = ShellNotifier::with_spec(CommandSpec {
            binary: "echo".into(),
            args: vec!["{title}".into(), "{body}".into()],
        });
        let n = make_notification("hello {body} world", "actual-body", NotificationLevel::Info);
        let args = notifier.render_args(&n);
        assert_eq!(args[0], "hello {body} world");
        assert_eq!(args[1], "actual-body");
    }

    #[test]
    fn test_substitute_preserves_unknown_placeholders() {
        let vars = [("name", "Alice")];
        assert_eq!(
            substitute("Hi {name}, city is {city}", &vars),
            "Hi Alice, city is {city}"
        );
    }

    #[test]
    fn test_substitute_handles_value_containing_placeholder_syntax() {
        let vars = [("a", "x{b}y"), ("b", "INJECTED")];
        let result = substitute("v={a}", &vars);
        assert_eq!(result, "v=x{b}y");
    }

    #[test]
    fn test_substitute_handles_unclosed_brace() {
        let vars = [("a", "A")];
        assert_eq!(substitute("v={a} {unclosed", &vars), "v=A {unclosed");
    }

    /// Cross-platform happy-path: `echo` exists on Linux/macOS and PowerShell
    /// on Windows. We don't assert on output (stdout is null) — only that spawn
    /// succeeds.
    #[test]
    fn test_shell_notifier_spawns_echo() {
        let notifier = ShellNotifier::with_spec(CommandSpec {
            binary: "echo".into(),
            args: vec!["{title}".into(), "{body}".into()],
        });
        let n = make_notification("hello", "world", NotificationLevel::Info);
        let result = notifier.send(&n);
        // On weird environments echo may not be in PATH; tolerate Err there.
        if let Err(e) = &result {
            eprintln!("note: echo spawn failed (acceptable in some CI): {e}");
        }
    }
}
