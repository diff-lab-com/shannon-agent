//! N3 per-kind trust store — the engine-side persistent allowlist behind the
//! mobile "always allow this category" scope (`shannon/approval/decide`
//! `scope: "kind"`).
//!
//! A grant records ONE kind string (the approval's tool name, matched
//! EXACTLY — no globs, no prefix semantics) and the epoch-ms grant time.
//! Grants live in `~/.shannon/trust/kinds.toml`:
//!
//! ```toml
//! [kinds]
//! "Bash" = 1767225600000
//! "Write" = 1767225800123
//! ```
//!
//! Trust semantics (the red lines this module enforces):
//! - **Category-scoped only**: a trusted kind auto-approves later permission
//!   checks whose tool name equals the kind byte-for-byte. There is no
//!   global always-allow and no cross-kind inheritance.
//! - **Revoke is immediate**: the store is the single shared in-process
//!   handle (`shared_store`), and every permission check consults it live;
//!   a revoke (route or file rewrite honored on next process start) takes
//!   effect on the very next request.
//! - **Nothing is fabricated**: the store holds only what a signed decision
//!   granted — never seeded from demo data or tool catalogs.
//!
//! The file is written atomically (temp file + rename) with `0600`
//! permissions inside a `0700` directory, following the credential-store
//! precedent. Parse failures degrade to an empty store with a warning —
//! a corrupt file must never wedge the permission gate.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Directory `~/.shannon/trust` (relative to the home dir).
const TRUST_DIR: &str = ".shannon/trust";
/// File `~/.shannon/trust/kinds.toml`.
const KINDS_FILE: &str = "kinds.toml";

/// The TOML document shape of `kinds.toml` (serde round-trips the file; the
/// `#[serde(default)]` keeps an empty/hand-pruned file parseable).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct KindsDoc {
    /// kind → epoch-ms grant time. BTreeMap keeps the file diff-stable.
    #[serde(default)]
    kinds: BTreeMap<String, u64>,
}

/// Shared per-kind trust state: one live map + its backing file path.
///
/// Cheap to clone (all state sits behind one `Arc`) — every
/// [`PermissionManager`](crate::permissions::PermissionManager) and the
/// api_server trust routes hold clones of the SAME handle, so a revoke via
/// the HTTP face is visible to every in-flight session immediately.
#[derive(Debug, Clone)]
pub struct KindTrustStore {
    inner: std::sync::Arc<Mutex<KindTrustInner>>,
}

#[derive(Debug)]
struct KindTrustInner {
    path: PathBuf,
    kinds: BTreeMap<String, u64>,
}

impl KindTrustStore {
    /// Open (or create) the store backed by `path`. A missing file is an
    /// empty store; a corrupt file is a warning + empty store (never an
    /// error — the permission gate must stay available).
    pub fn open(path: PathBuf) -> Self {
        let kinds = match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<KindsDoc>(&content) {
                Ok(doc) => doc.kinds,
                Err(e) => {
                    tracing::warn!(
                        "kind-trust store {} unreadable ({}); starting empty — \
                         re-grant or restore the file",
                        path.display(),
                        e
                    );
                    BTreeMap::new()
                }
            },
            Err(_) => BTreeMap::new(),
        };
        Self {
            inner: std::sync::Arc::new(Mutex::new(KindTrustInner { path, kinds })),
        }
    }

    /// The default engine store: `~/.shannon/trust/kinds.toml`. Unavailable
    /// home dirs fall back to the temp dir (matching the sessions-dir
    /// precedent) so the gate keeps working in stripped environments.
    pub fn default_path() -> PathBuf {
        match dirs::home_dir() {
            Some(home) => home.join(TRUST_DIR).join(KINDS_FILE),
            None => std::env::temp_dir().join(TRUST_DIR).join(KINDS_FILE),
        }
    }

    /// Grant (or re-stamp) one kind with `granted_at` epoch ms and persist.
    /// The in-memory map updates before the write, so the grant applies to
    /// the next permission check even if the disk write fails (best-effort
    /// durability, matching `persist_allow_rule`'s posture).
    pub fn grant(&self, kind: &str, granted_at_ms: u64) {
        let mut inner = lock(&self.inner);
        inner.kinds.insert(kind.to_string(), granted_at_ms);
        inner.persist(&inner.kinds.clone());
    }

    /// Remove one kind. `true` when a grant was actually dropped; `false`
    /// when the kind was not trusted (idempotent revoke). Persists on drop.
    pub fn revoke(&self, kind: &str) -> bool {
        let mut inner = lock(&self.inner);
        if inner.kinds.remove(kind).is_none() {
            return false;
        }
        inner.persist(&inner.kinds.clone());
        true
    }

    /// The trusted kind matching `tool_name` exactly, if any. Consulted live
    /// on every permission check — this is what makes revoke immediate.
    pub fn trusted_kind(&self, tool_name: &str) -> Option<String> {
        let inner = lock(&self.inner);
        inner
            .kinds
            .contains_key(tool_name)
            .then(|| tool_name.to_string())
    }

    /// Active grants, sorted by kind (the `GET /api/trust/kinds` body).
    pub fn list(&self) -> Vec<(String, u64)> {
        let inner = lock(&self.inner);
        inner.kinds.iter().map(|(k, t)| (k.clone(), *t)).collect()
    }
}

impl KindTrustInner {
    /// Atomically write the store: `0700` dir, `0600` temp file, rename.
    /// Failures are logged, never surfaced (the in-memory grant stands).
    fn persist(&self, kinds: &BTreeMap<String, u64>) {
        let doc = KindsDoc {
            kinds: kinds.clone(),
        };
        let Ok(body) = toml::to_string_pretty(&doc) else {
            tracing::warn!("kind-trust store serialize failed; keeping in-memory only");
            return;
        };
        if let Some(parent) = self.path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!("kind-trust dir create failed: {e}");
                return;
            }
            // Credential-store precedent: the trust dir holds privilege
            // grants, so it stays private to the owner.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        let tmp = self.path.with_extension("toml.tmp");
        let write = std::fs::write(&tmp, &body).and_then(|()| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
            }
            std::fs::rename(&tmp, &self.path)
        });
        if let Err(e) = write {
            tracing::warn!("kind-trust store write failed: {e}; keeping in-memory only");
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

/// Lock helper that recovers a poisoned mutex (a panic in one caller must
/// not permanently wedge the permission gate).
fn lock(inner: &Mutex<KindTrustInner>) -> std::sync::MutexGuard<'_, KindTrustInner> {
    inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Wall-clock epoch milliseconds — the grant timestamp source.
pub(crate) fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The process-wide shared store — ONE trust domain per engine process.
///
/// The api_server hands this same handle to every freshly-built session
/// [`PermissionManager`](crate::permissions::PermissionManager) and serves
/// the trust routes off it, so `POST /api/trust/revoke` lands in every
/// current and future session without restarts. Embedded hosts that want a
/// different scope build their own [`KindTrustStore`] and attach it via
/// [`crate::permissions::PermissionManager::set_kind_trust`].
pub fn shared_store() -> KindTrustStore {
    static SHARED: OnceLock<KindTrustStore> = OnceLock::new();
    SHARED
        .get_or_init(|| KindTrustStore::open(KindTrustStore::default_path()))
        .clone()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tmp_store(tag: &str) -> (KindTrustStore, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("shannon-trust-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kinds.toml");
        (KindTrustStore::open(path.clone()), path)
    }

    #[test]
    fn grant_persists_and_reloads_across_handles() {
        let (store, path) = tmp_store("persist");
        store.grant("Bash", 1_760_000_000_000);
        store.grant("Write", 1_760_000_000_100);

        // A fresh handle over the same file sees the grants (restart parity).
        let reopened = KindTrustStore::open(path.clone());
        assert_eq!(
            reopened.trusted_kind("Bash").as_deref(),
            Some("Bash"),
            "grants must survive a process restart via the toml file"
        );
        assert_eq!(
            reopened.list(),
            vec![
                ("Bash".to_string(), 1_760_000_000_000),
                ("Write".to_string(), 1_760_000_000_100),
            ]
        );

        // The file itself carries the frozen shape: a `[kinds]` table of
        // kind → epoch-ms entries. Simple kinds serialize as bare TOML keys;
        // kinds outside bare-key syntax are quoted automatically — both must
        // round-trip.
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("[kinds]"), "file shape: {body}");
        assert!(body.contains("Bash = 1760000000000"), "file shape: {body}");

        // A dotted kind (the mobile demo vocabulary, e.g. `browser.buy`)
        // stays valid TOML and parses back byte-for-byte.
        store.grant("browser.buy", 1_760_000_000_200);
        let reopened = KindTrustStore::open(path.clone());
        assert_eq!(
            reopened.trusted_kind("browser.buy").as_deref(),
            Some("browser.buy")
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn matching_is_exact_no_globs_no_prefix() {
        let (store, path) = tmp_store("exact");
        store.grant("Bash", 1);
        assert!(store.trusted_kind("Bash").is_some());
        assert!(
            store.trusted_kind("bash").is_none(),
            "case-sensitive exact match"
        );
        assert!(
            store.trusted_kind("BashTool").is_none(),
            "no prefix semantics"
        );
        assert!(
            store.trusted_kind("mcp__x__Bash").is_none(),
            "no substring semantics"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn revoke_is_immediate_and_idempotent() {
        let (store, path) = tmp_store("revoke");
        store.grant("Bash", 1);
        assert!(store.revoke("Bash"), "first revoke drops the grant");
        assert!(
            store.trusted_kind("Bash").is_none(),
            "revoke takes effect at once"
        );
        assert!(!store.revoke("Bash"), "second revoke is an honest no-op");

        // The removal persists: a fresh handle must not resurrect the grant.
        let reopened = KindTrustStore::open(path);
        assert!(reopened.trusted_kind("Bash").is_none());
        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join(format!("shannon-trust-test-{}-revoke", std::process::id())),
        );
    }

    #[test]
    fn corrupt_file_degrades_to_empty_store() {
        let dir =
            std::env::temp_dir().join(format!("shannon-trust-test-{}-corrupt", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kinds.toml");
        std::fs::write(&path, "{{{ not toml").unwrap();
        let store = KindTrustStore::open(path);
        assert!(
            store.list().is_empty(),
            "a corrupt file must never wedge the permission gate"
        );
        // And the store recovers: a grant rewrites the file cleanly.
        store.grant("Edit", 7);
        let reopened = KindTrustStore::open(dir.join("kinds.toml"));
        assert_eq!(reopened.trusted_kind("Edit").as_deref(), Some("Edit"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shared_store_is_one_handle_per_process() {
        let a = shared_store();
        let b = shared_store();
        // Grant through one clone is visible through the other — and both
        // see the real user file, so only assert the handle identity, not
        // the contents (the developer's machine may hold real grants).
        let probe = format!("__probe__{}", std::process::id());
        a.grant(&probe, 42);
        assert!(b.trusted_kind(&probe).is_some());
        assert!(b.revoke(&probe), "cleanup probe");
    }

    #[test]
    #[cfg(unix)]
    fn persisted_file_permissions_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let (store, path) = tmp_store("perms");
        store.grant("Bash", 1);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "trust file must be 0600");
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(dir_mode & 0o777, 0o700, "trust dir must be 0700");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
