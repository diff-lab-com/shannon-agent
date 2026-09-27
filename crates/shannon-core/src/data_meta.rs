//! # Data-directory version marker + compatibility gate (Phase 1)
//!
//! `~/.shannon/meta.json` records which Shannon version last wrote the data
//! directory (plus a per-store schema-version map for future migrations).
//! It is the guardrail that must exist **before** the first breaking data
//! change lands — it cannot be added retroactively.
//!
//! Phase 1 scope (2026-09):
//! - **Marker**: every entrypoint (CLI, `serve`, desktop) stamps the running
//!   version on startup. The stamp never moves backwards: a binary older
//!   than the recorded version leaves the recorded version intact, so the
//!   gate keeps working after a downgrade+re-upgrade cycle.
//! - **Gate**: a binary older than the recorded version refuses to run
//!   (CLI/`serve`) unless `SHANNON_ALLOW_DOWNGRADE=1` is set. The desktop
//!   logs a loud error and continues (it ships in the same bundle as its
//!   CLI sidecar, so self-downgrades are rare, and a desktop that refuses
//!   to open with no UI affordance is worse than a warned one). The CLI
//!   exempt `doctor` and `update` from the gate — they are exactly the
//!   tools needed to inspect or fix a gated install.
//! - **Backup primitive**: [`backup_before_migration`] copies store files
//!   into `<home>/backups/<from>-to-<to>-<ts>/`. No migration calls it yet;
//!   it exists so the first migration wires it instead of inventing one.
//!
//! Everything is best-effort and local: a missing or corrupt `meta.json`
//! degrades to "no marker" (record anew), never to a bricked install.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Marker file name, stored directly in the Shannon home directory.
pub const META_FILE: &str = "meta.json";

/// Schema-version map entries seeded by this module. Individual stores add
/// their own keys as migrations appear; entries are never decreased.
pub const SCHEMA_KEY_EVENTS: &str = "events";

/// Current event-log schema version, mirrored from `shannon_types`.
pub fn events_schema_version() -> u32 {
    shannon_types::events::EVENT_SCHEMA_VERSION
}

/// On-disk marker. `schema` maps a store name to its current schema version;
/// `BTreeMap` keeps the serialized form stable and diffable.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DataMeta {
    /// Version string (`x.y.z[-pre]`) of the binary that last wrote it.
    pub app_version: String,
    /// Per-store schema versions. Merged on write (max wins per key).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub schema: BTreeMap<String, u32>,
    /// Unix seconds of the last write (diagnostics only).
    #[serde(default)]
    pub updated_at_unix_secs: u64,
}

/// Outcome of comparing the recorded marker against the running binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compatibility {
    /// No marker (first run, or unreadable/corrupt file).
    NoMarker,
    /// Running binary is same version or newer than the marker.
    Compatible,
    /// Running binary is older than the marker — opening the data directory
    /// may read a format this binary does not understand.
    Downgrade {
        /// Version recorded in the marker (strictly newer than running).
        data_version: String,
    },
}

/// Resolve the Shannon home directory: `$SHANNON_HOME` override, else
/// `~/.shannon`. Same precedence as the other stores.
pub fn home() -> PathBuf {
    std::env::var_os("SHANNON_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".shannon")
        })
}

/// Marker path inside an explicit home (the `_in` functions are the pure,
/// testable core; the env-reading ones are thin wrappers).
pub fn meta_path_in(home: &Path) -> PathBuf {
    home.join(META_FILE)
}

/// Marker path under the resolved home.
pub fn meta_path() -> PathBuf {
    meta_path_in(&home())
}

/// Read the marker; `None` when absent or unparseable (a corrupt marker is
/// logged and treated as absent — never a hard error).
pub fn read_from(home: &Path) -> Option<DataMeta> {
    let bytes = match std::fs::read(meta_path_in(home)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(error = %e, "data_meta: could not read meta.json");
            return None;
        }
    };
    match serde_json::from_slice::<DataMeta>(&bytes) {
        Ok(meta) => Some(meta),
        Err(e) => {
            tracing::warn!(error = %e, "data_meta: meta.json is corrupt — treating as absent");
            None
        }
    }
}

/// Read the marker from the resolved home.
pub fn read() -> Option<DataMeta> {
    read_from(&home())
}

/// Compare the marker against the running version.
pub fn check_against(meta: Option<&DataMeta>, running: &str) -> Compatibility {
    let Some(meta) = meta else {
        return Compatibility::NoMarker;
    };
    if is_downgrade(&meta.app_version, running) {
        Compatibility::Downgrade {
            data_version: meta.app_version.clone(),
        }
    } else {
        Compatibility::Compatible
    }
}

/// Compare against the marker in the resolved home.
pub fn check() -> Compatibility {
    check_against(read().as_ref(), env!("CARGO_PKG_VERSION"))
}

/// True when `data_version` was written by a binary strictly newer than
/// `running`. Unparseable versions never count as a downgrade (an unknown
/// scheme must not brick the install) — callers warn instead.
///
/// Comparison is `(major, minor, patch)` with one prerelease rule: equal
/// triples where the data version is a release (`0.12.0`) and the running
/// binary is a prerelease (`0.12.0-rc.1`) count as a downgrade, because the
/// release may carry migrations the rc never saw. Prerelease-vs-prerelease
/// of the same triple is treated as compatible (best-effort).
pub fn is_downgrade(data_version: &str, running: &str) -> bool {
    let (Some(data), Some(run)) = (parse_version(data_version), parse_version(running)) else {
        return false;
    };
    match data.triple.cmp(&run.triple) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => data.is_release && !run.is_release,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedVersion {
    triple: (u64, u64, u64),
    /// False when a `-pre` suffix is present.
    is_release: bool,
}

fn parse_version(version: &str) -> Option<ParsedVersion> {
    let version = version.trim().trim_start_matches('v');
    let (core, _pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(ParsedVersion {
        triple: (major, minor, patch),
        is_release: _pre.is_none(),
    })
}

/// Stamp the running version into the marker (resolved home). Best-effort:
/// I/O failures are logged, never fatal. The recorded version and every
/// schema entry only ever move **forward** — a downgrade must not clobber a
/// newer marker, or the gate stops protecting the directory after a
/// downgrade+re-upgrade cycle.
pub fn record_in(home: &Path, running: &str) {
    let mut meta = read_from(home).unwrap_or_default();
    let running_parsed = parse_version(running);
    let recorded_parsed = parse_version(&meta.app_version);
    let keep_recorded = match (recorded_parsed, running_parsed) {
        (Some(rec), Some(run)) => rec.triple > run.triple,
        // Unparseable recorded version: leave it rather than overwrite with
        // something we can't reason about later.
        (Some(_), None) => true,
        _ => false,
    };
    if !keep_recorded {
        meta.app_version = running.to_string();
    }
    meta.schema
        .entry(SCHEMA_KEY_EVENTS.to_string())
        .and_modify(|v| *v = (*v).max(events_schema_version()))
        .or_insert(events_schema_version());
    meta.updated_at_unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = meta_path_in(home);
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(error = %e, "data_meta: could not create Shannon home");
            return;
        }
    }
    match serde_json::to_vec_pretty(&meta) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                tracing::warn!(error = %e, "data_meta: could not write meta.json");
            }
        }
        Err(e) => tracing::warn!(error = %e, "data_meta: could not serialize meta.json"),
    }
}

/// Stamp the running version into the resolved home's marker.
pub fn record_current_version() {
    record_in(&home(), env!("CARGO_PKG_VERSION"));
}

/// Opinionated startup gate for headless entrypoints (CLI, `serve`):
///
/// - Downgrade without `SHANNON_ALLOW_DOWNGRADE` → `Err` with a message the
///   caller prints and exits on.
/// - Downgrade with the override → warn, then record.
/// - Anything else → record.
///
/// Success always means "safe to proceed AND marker is up to date".
pub fn check_and_record() -> Result<(), String> {
    let running = env!("CARGO_PKG_VERSION");
    match check_against(read().as_ref(), running) {
        Compatibility::Downgrade { data_version }
            if std::env::var_os("SHANNON_ALLOW_DOWNGRADE").is_none() =>
        {
            return Err(format!(
                "refusing to run: the Shannon data directory was last written by \
                 version {data_version}, which is newer than this binary ({running}).\n\
                 Older binaries may not understand newer data formats and can corrupt\n\
                 sessions, inbox, or memory stores.\n\
                 Proceed anyway with:  SHANNON_ALLOW_DOWNGRADE=1 shannon …\n\
                 Or check what changed:  shannon doctor --deep"
            ));
        }
        Compatibility::Downgrade { data_version } => {
            tracing::warn!(
                data_version = %data_version,
                running = %running,
                "SHANNON_ALLOW_DOWNGRADE set — running an older binary against a newer data directory"
            );
        }
        _ => {}
    }
    record_current_version();
    Ok(())
}

/// Copy store files into `<home>/backups/<from>-to-<to>-<unixts>/` before a
/// migration mutates them. Files are copied; directories are copied
/// recursively (their relative tree is preserved). Missing inputs are
/// skipped silently; a failed copy aborts with the io error — a partial
/// backup must not look complete. Returns the backup directory.
///
/// No callers yet (Phase 1 has no migrations); the first migration must
/// call this **before** its first write.
pub fn backup_before_migration_in(
    home: &Path,
    from_version: &str,
    to_version: &str,
    paths: &[&Path],
) -> std::io::Result<PathBuf> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dir = home
        .join("backups")
        .join(format!("{from_version}-to-{to_version}-{ts}"));
    std::fs::create_dir_all(&dir)?;
    for path in paths {
        if !path.exists() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_owned())
            .unwrap_or_else(|| std::ffi::OsString::from("unnamed"));
        let dest = dir.join(&name);
        if path.is_dir() {
            copy_dir_recursive(path, &dest)?;
        } else {
            std::fs::copy(path, &dest)?;
        }
    }
    Ok(dir)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join(".shannon");
        std::fs::create_dir_all(&home).expect("mkdir");
        (dir, home)
    }

    #[test]
    fn no_marker_is_compatible_and_recorded() {
        let (_dir, home) = temp_home();
        assert_eq!(
            check_against(read_from(&home).as_ref(), "0.11.0"),
            Compatibility::NoMarker
        );
        record_in(&home, "0.11.0");
        let meta = read_from(&home).expect("marker written");
        assert_eq!(meta.app_version, "0.11.0");
        assert_eq!(
            meta.schema.get(SCHEMA_KEY_EVENTS),
            Some(&events_schema_version())
        );
        assert_eq!(
            check_against(read_from(&home).as_ref(), "0.11.0"),
            Compatibility::Compatible
        );
    }

    #[test]
    fn downgrade_detected_by_triple() {
        let meta = DataMeta {
            app_version: "0.12.0".into(),
            ..Default::default()
        };
        assert_eq!(
            check_against(Some(&meta), "0.11.9"),
            Compatibility::Downgrade {
                data_version: "0.12.0".into()
            }
        );
    }

    #[test]
    fn release_data_vs_rc_binary_is_downgrade() {
        // Data written by the 0.12.0 release, binary is an 0.12.0-rc.1.
        let meta = DataMeta {
            app_version: "0.12.0".into(),
            ..Default::default()
        };
        assert_eq!(
            check_against(Some(&meta), "0.12.0-rc.1"),
            Compatibility::Downgrade {
                data_version: "0.12.0".into()
            }
        );
        // …but rc data vs release binary is fine, and equal rcs are fine.
        let rc_meta = DataMeta {
            app_version: "0.12.0-rc.1".into(),
            ..Default::default()
        };
        assert_eq!(
            check_against(Some(&rc_meta), "0.12.0"),
            Compatibility::Compatible
        );
        assert_eq!(
            check_against(Some(&rc_meta), "0.12.0-rc.1"),
            Compatibility::Compatible
        );
    }

    #[test]
    fn unparseable_versions_never_gate() {
        assert!(!is_downgrade("not-a-version", "0.11.0"));
        assert!(!is_downgrade("0.12.0", "also-weird"));
        assert!(!is_downgrade("1.2.3.4", "0.11.0")); // four components → refuse to judge
    }

    #[test]
    fn v_prefix_and_whitespace_tolerated() {
        assert!(!is_downgrade("v0.10.0", "0.11.0"));
        assert!(is_downgrade(" v0.12.0 ", "0.11.0"));
    }

    #[test]
    fn record_never_moves_backwards() {
        let (_dir, home) = temp_home();
        record_in(&home, "0.12.0");
        // An older binary must not clobber the newer marker.
        record_in(&home, "0.11.0");
        assert_eq!(read_from(&home).expect("marker").app_version, "0.12.0");
        // …and a newer binary does update it.
        record_in(&home, "0.13.0");
        assert_eq!(read_from(&home).expect("marker").app_version, "0.13.0");
    }

    #[test]
    fn record_merges_schema_entries_never_down() {
        let (_dir, home) = temp_home();
        record_in(&home, "0.11.0");
        // Simulate a future store bumping its schema entry beyond what this
        // binary seeds.
        let path = meta_path_in(&home);
        let mut meta = read_from(&home).expect("marker");
        meta.schema.insert("inbox".into(), 3);
        std::fs::write(&path, serde_json::to_vec(&meta).unwrap()).unwrap();
        record_in(&home, "0.11.0");
        let meta = read_from(&home).expect("marker");
        assert_eq!(meta.schema.get("inbox"), Some(&3));
        assert!(meta.schema.contains_key(SCHEMA_KEY_EVENTS));
    }

    #[test]
    fn corrupt_marker_treated_as_absent() {
        let (_dir, home) = temp_home();
        std::fs::write(meta_path_in(&home), b"{ not json").unwrap();
        assert!(read_from(&home).is_none());
        assert_eq!(check_against(None, "0.11.0"), Compatibility::NoMarker);
    }

    #[test]
    fn backup_copies_files_and_dirs_skips_missing() {
        let (_dir, home) = temp_home();
        let store = home.join("providers.toml");
        std::fs::write(&store, b"key = 'value'").unwrap();
        let dir_store = home.join("sessions");
        std::fs::create_dir_all(dir_store.join("abc")).unwrap();
        std::fs::write(dir_store.join("abc").join("events.jsonl"), b"{}\n").unwrap();
        let missing = home.join("does-not-exist.db");

        let backup = backup_before_migration_in(
            &home,
            "0.11.0",
            "0.12.0",
            &[store.as_path(), dir_store.as_path(), missing.as_path()],
        )
        .expect("backup");

        assert!(backup.starts_with(home.join("backups")));
        assert!(backup.to_string_lossy().contains("0.11.0-to-0.12.0"));
        assert_eq!(
            std::fs::read_to_string(backup.join("providers.toml")).unwrap(),
            "key = 'value'"
        );
        assert_eq!(
            std::fs::read_to_string(backup.join("sessions/abc/events.jsonl")).unwrap(),
            "{}\n"
        );
    }
}
