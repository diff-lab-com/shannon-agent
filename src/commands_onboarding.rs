//! Onboarding seed (#75) — first-run sample data.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! Writes three generic sample tasks to `.claude/tasks/` on first run so
//! the Tasks / Today surfaces aren't empty. Idempotent: re-running on a
//! directory that already holds any `*.json` file is a no-op.

use serde::{Deserialize, Serialize};

use crate::commands::chrono_timestamp;

/// Report returned by `seed_sample_data` so the UI can tell the user what landed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedReport {
    /// Number of sample task files written. Zero when tasks already existed.
    pub tasks_seeded: usize,
}

/// Three onboarding tasks. IDs are stable so re-seeding is a no-op.
const SAMPLE_TASKS: &[(&str, &str, &str, &str, &[&str])] = &[
    (
        "sample-welcome-1",
        "Read the project README",
        "Open README.md and skim the architecture overview to get oriented.",
        "todo",
        &["getting-started"],
    ),
    (
        "sample-welcome-2",
        "Sketch a quick design",
        "Capture your initial idea as a 1-page note — what problem, what user, what shape.",
        "todo",
        &["design", "draft"],
    ),
    (
        "sample-welcome-3",
        "Run the test suite",
        "Execute `cargo test --workspace` (or the project's documented command) to confirm a clean baseline.",
        "in-progress",
        &["validation"],
    ),
];

/// Write sample tasks to `.claude/tasks/` on first run.
///
/// No-op when the directory already contains any `*.json` file (idempotent).
/// Creates the directory if missing. Returns the count of tasks written.
#[tauri::command]
pub async fn seed_sample_data() -> Result<SeedReport, String> {
    seed_sample_data_in(std::path::Path::new(".claude/tasks")).await
}

/// Path-parameterised core. The Tauri command above hard-codes `.claude/tasks`
/// (the location `list_tasks` reads from); tests call this with a tempdir so
/// they don't race on the process working directory.
async fn seed_sample_data_in(tasks_dir: &std::path::Path) -> Result<SeedReport, String> {
    std::fs::create_dir_all(tasks_dir).map_err(|e| format!("create tasks dir: {e}"))?;

    // Idempotent guard — if anything is already there, don't seed.
    let has_existing = std::fs::read_dir(tasks_dir)
        .map_err(|e| format!("read tasks dir: {e}"))?
        .filter_map(Result::ok)
        .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"));
    if has_existing {
        return Ok(SeedReport { tasks_seeded: 0 });
    }

    let now = chrono_timestamp();
    let due_in_24h = now + 24 * 60 * 60;

    for (id, title, description, status, tags) in SAMPLE_TASKS.iter().copied() {
        let body = serde_json::json!({
            "id": id,
            "title": title,
            "description": description,
            "status": status,
            "priority": "medium",
            "tags": tags,
            "dueDate": due_in_24h,
            "createdAt": now,
            "activeForm": match status {
                "in-progress" => Some("Working on sample task".to_string()),
                _ => None,
            },
        });
        let path = tasks_dir.join(format!("{id}.json"));
        let pretty = serde_json::to_string_pretty(&body)
            .map_err(|e| format!("serialize sample task {id}: {e}"))?;
        std::fs::write(&path, pretty)
            .map_err(|e| format!("write sample task {}: {e}", path.display()))?;
    }

    Ok(SeedReport {
        tasks_seeded: SAMPLE_TASKS.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seed_sample_data_writes_three_tasks() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tasks_dir = tmp.path().join(".claude/tasks");

        let rt = tokio::runtime::Runtime::new().expect("rt");
        let report = rt.block_on(seed_sample_data_in(&tasks_dir)).expect("seed");

        assert_eq!(report.tasks_seeded, 3);

        let entries: Vec<_> = std::fs::read_dir(&tasks_dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 3, "exactly three sample tasks written");

        let mut ids = Vec::new();
        for entry in &entries {
            let body: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(entry.path()).expect("read"))
                    .expect("parse");
            ids.push(body["id"].as_str().unwrap_or("").to_string());
            assert!(
                body["title"].as_str().is_some(),
                "title field present on {:?}",
                entry.path()
            );
        }
        ids.sort();
        assert_eq!(
            ids,
            vec![
                "sample-welcome-1".to_string(),
                "sample-welcome-2".to_string(),
                "sample-welcome-3".to_string(),
            ]
        );
    }

    #[test]
    fn test_seed_sample_data_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tasks_dir = tmp.path().join(".claude/tasks");

        let rt = tokio::runtime::Runtime::new().expect("rt");
        let first = rt
            .block_on(seed_sample_data_in(&tasks_dir))
            .expect("seed 1");
        assert_eq!(first.tasks_seeded, 3);

        let second = rt
            .block_on(seed_sample_data_in(&tasks_dir))
            .expect("seed 2");
        assert_eq!(second.tasks_seeded, 0);

        let count = std::fs::read_dir(&tasks_dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .count();
        assert_eq!(count, 3, "no duplicate files after re-seed");
    }
}
