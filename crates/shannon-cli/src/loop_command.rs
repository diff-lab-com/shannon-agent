//! CLI surface for the `/loop` and `/schedule` commands.
//!
//! Mirrors Claude Code's slash-command ergonomics but routes through
//! Shannon's [`shannon_core::scheduled_routines`] machinery (the REPL's
//! `loop_engine` is the canonical TUI entry point; this module exposes a
//! non-interactive entry point for the CLI binary so CI / scripts can
//! schedule routines without booting the TUI).
//!
//! ## Subcommands
//!
//! - `shannon loop start <interval> <prompt>` — schedule a recurring task
//! - `shannon loop once <interval> <prompt>`  — schedule a one-shot
//! - `shannon loop list`                      — print every scheduled routine
//! - `shannon loop remove <id-or-name>`       — cancel a scheduled routine
//! - `shannon loop status`                    — show counts + next-fire stats
//!
//! ## State
//!
//! Reads and writes the same `~/.shannon/routines.json` (legacy) or
//! `~/.shannon/scheduled-tasks/<slug>-<id>/{SKILL.md, task.json}` (Sprint 1
//! layout) files used by the rest of the scheduled-routines subsystem.
//! For the CLI we read/write via
//! [`shannon_core::scheduled_routines::RoutineManager`] which transparently
//! handles both formats.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Utc};

use shannon_core::scheduled_routines::{RoutineManager, ScheduledRoutine, TriggerType};

/// Canonical interval parser. Accepts:
///
/// - `30s` (seconds)
/// - `5m` (minutes)
/// - `2h` (hours)
/// - `1d` (days)
/// - `1w` (weeks — converted to days)
/// - bare integer (seconds, for backward compat with Claude Code's `/loop`)
pub fn parse_interval(input: &str) -> Result<ChronoDuration> {
    let s = input.trim();
    if s.is_empty() {
        anyhow::bail!("interval cannot be empty");
    }
    // bare integer => seconds (Claude Code parity)
    if let Ok(n) = s.parse::<u64>() {
        return Ok(ChronoDuration::seconds(n as i64));
    }
    let split = s
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| anyhow::anyhow!("missing unit suffix in interval '{s}'"))?;
    let (num, unit) = s.split_at(split);
    let n: i64 = num
        .parse()
        .with_context(|| format!("invalid interval number '{num}'"))?;
    if n < 0 {
        anyhow::bail!("interval cannot be negative");
    }
    let dur = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => ChronoDuration::seconds(n),
        "m" | "min" | "mins" | "minute" | "minutes" => ChronoDuration::minutes(n),
        "h" | "hr" | "hrs" | "hour" | "hours" => ChronoDuration::hours(n),
        "d" | "day" | "days" => ChronoDuration::days(n),
        "w" | "wk" | "week" | "weeks" => ChronoDuration::weeks(n),
        other => anyhow::bail!("unknown interval unit '{other}' (use s/m/h/d/w)"),
    };
    Ok(dur)
}

/// Outcome of a CLI subcommand — print this and exit.
#[derive(Debug)]
pub enum CliOutput {
    /// Routine successfully created.
    Created { id: String, name: String },
    /// Routine successfully removed.
    Removed { id: String },
    /// Routine listing.
    List(Vec<ScheduledRoutine>),
    /// Summary suitable for `status`.
    Status(StatusReport),
}

#[derive(Debug)]
pub struct StatusReport {
    pub total: usize,
    pub enabled: usize,
    pub next_fire: Option<(String, chrono::DateTime<Utc>)>,
}

/// CLI subcommand verb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopCommand {
    /// Schedule a recurring task (`/loop <interval> <prompt>`).
    Start {
        interval: String,
        prompt: String,
        max_fires: Option<u32>,
    },
    /// Schedule a one-shot (`/loop once <interval> <prompt>`).
    Once { interval: String, prompt: String },
    /// Print every scheduled routine.
    List,
    /// Cancel a routine by id (8-char prefix) or name.
    Remove(String),
    /// Show counts + next-fire stats.
    Status,
}

impl FromStr for LoopCommand {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let mut parts = s.split_whitespace();
        let verb = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing verb (start|once|list|remove|status)"))?;
        match verb {
            "start" => {
                let interval = parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing interval"))?
                    .to_string();
                let prompt: String = parts.collect::<Vec<_>>().join(" ");
                if prompt.is_empty() {
                    anyhow::bail!("missing prompt");
                }
                Ok(LoopCommand::Start {
                    interval,
                    prompt,
                    max_fires: None,
                })
            }
            "once" => {
                let interval = parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing interval"))?
                    .to_string();
                let prompt: String = parts.collect::<Vec<_>>().join(" ");
                if prompt.is_empty() {
                    anyhow::bail!("missing prompt");
                }
                Ok(LoopCommand::Once { interval, prompt })
            }
            "list" | "ls" => Ok(LoopCommand::List),
            "remove" | "rm" | "cancel" => {
                let id = parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing id-or-name"))?
                    .to_string();
                Ok(LoopCommand::Remove(id))
            }
            "status" => Ok(LoopCommand::Status),
            other => {
                anyhow::bail!("unknown verb '{other}' (expected start|once|list|remove|status)")
            }
        }
    }
}

/// Load a [`RoutineManager`] from `path`. Missing file → empty manager.
fn load_manager(path: &Path) -> Result<RoutineManager> {
    if !path.exists() {
        return Ok(RoutineManager::new());
    }
    RoutineManager::load_from_file(path)
        .with_context(|| format!("loading routines from {}", path.display()))
}

/// Persist a [`RoutineManager`] to `path`.
fn save_manager(manager: &RoutineManager, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent dir {}", parent.display()))?;
    }
    manager
        .save_to_file(path)
        .with_context(|| format!("saving routines to {}", path.display()))
}

/// Apply a [`LoopCommand`] against the on-disk routines file.
///
/// This is the unit-testable core — the binary entry point just parses
/// argv and prints the [`CliOutput`].
pub fn apply_command(storage: &Path, cmd: &LoopCommand) -> Result<CliOutput> {
    let mut manager = load_manager(storage)?;
    let out = match cmd {
        LoopCommand::Start {
            interval,
            prompt,
            max_fires,
        } => {
            let dur = parse_interval(interval)?;
            let secs = dur.num_seconds().max(0) as u64;
            let mut routine = ScheduledRoutine::new(derive_name(prompt), prompt.clone(), secs);
            routine.trigger_type = TriggerType::Interval;
            routine.max_fires = *max_fires;
            let id = routine.id.clone();
            let name = routine.name.clone();
            manager.add(routine);
            CliOutput::Created { id, name }
        }
        LoopCommand::Once { interval, prompt } => {
            let dur = parse_interval(interval)?;
            let secs = dur.num_seconds().max(0) as u64;
            let mut routine = ScheduledRoutine::new(derive_name(prompt), prompt.clone(), secs);
            routine.trigger_type = TriggerType::Interval;
            routine.max_fires = Some(1);
            let id = routine.id.clone();
            let name = routine.name.clone();
            manager.add(routine);
            CliOutput::Created { id, name }
        }
        LoopCommand::List => {
            let owned: Vec<ScheduledRoutine> = manager.list().into_iter().cloned().collect();
            CliOutput::List(owned)
        }
        LoopCommand::Remove(id_or_name) => {
            // Try exact id first, then name.
            let by_id = manager.remove(id_or_name);
            let removed = if by_id.is_some() {
                by_id
            } else {
                let name_match = manager
                    .list()
                    .into_iter()
                    .find(|r| r.name == *id_or_name)
                    .map(|r| r.id.clone());
                match name_match {
                    Some(id) => manager.remove(&id),
                    None => None,
                }
            }
            .ok_or_else(|| anyhow::anyhow!("no routine matches '{id_or_name}'"))?;
            CliOutput::Removed { id: removed.id }
        }
        LoopCommand::Status => {
            let all: Vec<ScheduledRoutine> = manager.list().into_iter().cloned().collect();
            let total = all.len();
            let enabled = all.iter().filter(|r| r.enabled).count();
            let next_fire = all
                .iter()
                .filter(|r| r.enabled)
                .filter_map(|r| r.next_fire_at.map(|t| (r.id.clone(), t)))
                .min_by_key(|(_, t)| *t);
            CliOutput::Status(StatusReport {
                total,
                enabled,
                next_fire,
            })
        }
    };
    save_manager(&manager, storage)?;
    Ok(out)
}

/// Convenience: apply against the user's default storage path.
#[allow(dead_code)] // KEEP: public API for future CLI entry point
pub fn apply_default(cmd: &LoopCommand) -> Result<CliOutput> {
    apply_command(&RoutineManager::default_storage_path(), cmd)
}

/// Format a [`CliOutput`] for stdout (one line, plain text).
pub fn format_output(out: &CliOutput) -> String {
    match out {
        CliOutput::Created { id, name } => {
            format!("created routine id={id} name={name:?}")
        }
        CliOutput::Removed { id } => format!("removed routine id={id}"),
        CliOutput::List(routines) => {
            if routines.is_empty() {
                return "no scheduled routines".into();
            }
            let mut s = String::from("id        enabled  next_fire              name\n");
            for r in routines {
                let nf = r
                    .next_fire_at
                    .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_else(|| "-".into());
                s.push_str(&format!(
                    "{id:<9} {en:<7}  {nf:<20} {name}\n",
                    id = r.id,
                    en = if r.enabled { "yes" } else { "no" },
                    name = r.name,
                ));
            }
            s
        }
        CliOutput::Status(s) => {
            let nf = s
                .next_fire
                .as_ref()
                .map(|(id, t)| format!("{id} at {}", t.format("%Y-%m-%dT%H:%M:%SZ")))
                .unwrap_or_else(|| "-".into());
            format!("total={} enabled={} next_fire={}", s.total, s.enabled, nf)
        }
    }
}

/// Derive a short, filesystem-safe name from a prompt (used as the
/// routine's `name` field when the user doesn't supply one).
fn derive_name(prompt: &str) -> String {
    let trimmed = prompt.trim();
    let first: String = trimmed.chars().take(40).collect();
    if first.is_empty() {
        "routine".into()
    } else {
        first
    }
}

/// Resolve the storage path, allowing CLI overrides for tests.
#[allow(dead_code)] // KEEP: public API for future CLI flags
pub fn resolve_storage_path(custom: Option<PathBuf>) -> PathBuf {
    custom.unwrap_or_else(RoutineManager::default_storage_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interval_bare_number_is_seconds() {
        assert_eq!(parse_interval("60").unwrap(), ChronoDuration::seconds(60));
    }

    #[test]
    fn parse_interval_units() {
        assert_eq!(parse_interval("5m").unwrap(), ChronoDuration::minutes(5));
        assert_eq!(parse_interval("2h").unwrap(), ChronoDuration::hours(2));
        assert_eq!(parse_interval("1d").unwrap(), ChronoDuration::days(1));
        assert_eq!(parse_interval("1w").unwrap(), ChronoDuration::weeks(1));
        assert_eq!(parse_interval("30s").unwrap(), ChronoDuration::seconds(30));
    }

    #[test]
    fn parse_interval_rejects_garbage() {
        assert!(parse_interval("").is_err());
        assert!(parse_interval("5x").is_err());
        assert!(parse_interval("abc").is_err());
        assert!(parse_interval("-1m").is_err());
    }

    #[test]
    fn from_str_parses_verbs() {
        let cmd = LoopCommand::from_str("start 5m echo hi").unwrap();
        match cmd {
            LoopCommand::Start {
                interval, prompt, ..
            } => {
                assert_eq!(interval, "5m");
                assert_eq!(prompt, "echo hi");
            }
            _ => panic!("expected Start"),
        }
        let cmd = LoopCommand::from_str("once 30s check").unwrap();
        match cmd {
            LoopCommand::Once { interval, prompt } => {
                assert_eq!(interval, "30s");
                assert_eq!(prompt, "check");
            }
            _ => panic!("expected Once"),
        }
        assert_eq!(LoopCommand::from_str("list").unwrap(), LoopCommand::List);
        assert_eq!(
            LoopCommand::from_str("remove abc12345").unwrap(),
            LoopCommand::Remove("abc12345".into())
        );
        assert_eq!(
            LoopCommand::from_str("status").unwrap(),
            LoopCommand::Status
        );
    }

    #[test]
    fn from_str_rejects_unknown_verbs() {
        assert!(LoopCommand::from_str("nuke everything").is_err());
        assert!(LoopCommand::from_str("").is_err());
        assert!(LoopCommand::from_str("start").is_err()); // missing args
    }

    #[test]
    fn apply_start_persists_routine() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join("routines.json");

        let out = apply_command(
            &storage,
            &LoopCommand::Start {
                interval: "5m".into(),
                prompt: "check the deploy".into(),
                max_fires: None,
            },
        )
        .unwrap();
        match out {
            CliOutput::Created { id, .. } => assert_eq!(id.len(), 8),
            _ => panic!("expected Created"),
        }
        assert!(storage.exists());

        // Reload via second invocation and confirm persistence.
        let out2 = apply_command(&storage, &LoopCommand::List).unwrap();
        match out2 {
            CliOutput::List(rs) => assert_eq!(rs.len(), 1),
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn apply_remove_uses_id_then_name() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join("routines.json");

        apply_command(
            &storage,
            &LoopCommand::Start {
                interval: "1m".into(),
                prompt: "ping".into(),
                max_fires: None,
            },
        )
        .unwrap();

        let out = apply_command(&storage, &LoopCommand::List).unwrap();
        let id = match out {
            CliOutput::List(rs) => rs[0].id.clone(),
            _ => panic!("expected List"),
        };

        // Remove by id.
        apply_command(&storage, &LoopCommand::Remove(id)).unwrap();
        let out = apply_command(&storage, &LoopCommand::List).unwrap();
        match out {
            CliOutput::List(rs) => assert!(rs.is_empty()),
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn apply_remove_unknown_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join("routines.json");
        assert!(apply_command(&storage, &LoopCommand::Remove("nope".into())).is_err());
    }

    #[test]
    fn status_reports_totals() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join("routines.json");

        apply_command(
            &storage,
            &LoopCommand::Start {
                interval: "1m".into(),
                prompt: "a".into(),
                max_fires: None,
            },
        )
        .unwrap();
        let out = apply_command(&storage, &LoopCommand::Status).unwrap();
        match out {
            CliOutput::Status(s) => {
                assert_eq!(s.total, 1);
                assert_eq!(s.enabled, 1);
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn format_output_handles_empty_list() {
        let s = format_output(&CliOutput::List(vec![]));
        assert_eq!(s, "no scheduled routines");
    }

    #[test]
    fn format_output_created_and_removed() {
        assert_eq!(
            format_output(&CliOutput::Created {
                id: "abc".into(),
                name: "x".into()
            }),
            "created routine id=abc name=\"x\""
        );
        assert_eq!(
            format_output(&CliOutput::Removed { id: "abc".into() }),
            "removed routine id=abc"
        );
    }
}
