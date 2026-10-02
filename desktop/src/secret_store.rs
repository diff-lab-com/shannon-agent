//! OS-keyring credential storage behind a [`SecretStore`] trait seam
//! (R7-④ batch 2, adversarial-review A8): MCP OAuth tokens and data-source
//! credentials move out of the plaintext `settings.json` / `data-sources/*.toml`
//! files into the OS keyring, with the plaintext kept only as a migration
//! fallback.
//!
//! # Key namespace
//!
//! Every key is the fully-qualified string `shannon/<domain>/<name>`:
//!
//! | domain       | key                            | payload                          |
//! |--------------|--------------------------------|----------------------------------|
//! | `mcp-oauth`  | `shannon/mcp-oauth/<server>`   | JSON `McpStoredOAuth` token block |
//! | `datasource` | `shannon/datasource/<slug>`    | JSON map of secret field → value  |
//!
//! The [`KeyringStore`] maps a key onto the `keyring` crate's
//! `(service, account)` pair by splitting at the **first** `/`: service
//! `"shannon"`, account `"<domain>/<name>"`. The gateway social connections
//! (the pre-existing keyring tenants, `commands_connections.rs`) key their
//! entries `"<platform>/<slot>"` with platform ∈ {slack, telegram, discord,
//! matrix, whatsapp, wecom, feishu, dingtalk} and a `shannon-gateway` default
//! — the `"shannon"` service never collides with any of them, and the
//! provider API keys use the file-based `credential_manager`, not the
//! keyring at all.
//!
//! # Degradation is never silent
//!
//! [`init_global`] probes the keyring once at startup (write → read → delete
//! a probe entry). When the probe fails, the process falls back to the F3
//! `0600` plaintext helper — and emits one structured
//! [`tracing::warn!`] **per affected domain** so the downgrade is visible in
//! logs. The UI mirrors the same fact: the MCP Servers and Data Sources
//! pages each render a "credential storage" line driven by the
//! `"keyring"` / `"plaintext_file"` wire tokens produced by
//! [`storage_mode`].
//!
//! # Tests never touch the real keyring
//!
//! CI has no Secret Service. Every test injects a [`MockSecretStore`] through
//! the explicit `*_with_store` / `*_{from,to,in}_with_store` parameter
//! variants of the domain functions; the production wrappers resolve
//! [`global()`], which stays `None` in tests (only the Tauri `setup` calls
//! [`init_global`]). A test that reached the real keyring would be wrong by
//! construction — the mock is the only store implementation tests may use.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

/// keyring `service` for every entry this module writes. Deliberately a
/// namespace of its own — see the module docs for the collision analysis
/// against the gateway connections' keys.
pub const SERVICE: &str = "shannon";

/// Probe entry, written/read/deleted once at startup to decide the storage
/// backend. A dedicated `probe` domain so a leftover probe entry can never be
/// mistaken for a migrated credential.
const PROBE_KEY: &str = "shannon/probe/desktop-startup";

/// Storage seam for one secret value. Keys are fully qualified
/// `shannon/<domain>/<name>` strings — implementations decide the physical
/// mapping (the production impl: OS keyring `(service, account)`).
pub trait SecretStore: Send + Sync {
    /// Read a secret. `Ok(None)` = no entry (not an error).
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    /// Write (create or overwrite) a secret.
    fn put(&self, key: &str, value: &str) -> Result<(), String>;
    /// Delete a secret. Idempotent — deleting a missing entry succeeds.
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// Fully-qualified keyring key for one MCP server's OAuth token block.
pub fn mcp_oauth_key(server: &str) -> String {
    format!("{SERVICE}/mcp-oauth/{server}")
}

/// Fully-qualified keyring key for one data source's secret fields.
pub fn datasource_key(slug: &str) -> String {
    format!("{SERVICE}/datasource/{slug}")
}

/// Split a fully-qualified key into the keyring crate's `(service, account)`
/// pair — split at the first `/`, exactly like the gateway connections'
/// `split_secret_key`. A key without `/` is a caller bug (our builders always
/// produce the `shannon/` prefix); it still maps safely to
/// `(SERVICE, key)`.
fn split_key(key: &str) -> (&str, &str) {
    match key.find('/') {
        Some(idx) => (&key[..idx], &key[idx + 1..]),
        None => (SERVICE, key),
    }
}

/// Production implementation: the OS keyring via the `keyring` crate
/// (apple-native / windows-native / sync-secret-service, matching the
/// gateway connections' backend so one OS credential store serves all
/// of Shannon's keyring entries).
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let (service, account) = split_key(key);
        let entry = keyring::Entry::new(service, account).map_err(err)?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(err(e)),
        }
    }

    fn put(&self, key: &str, value: &str) -> Result<(), String> {
        let (service, account) = split_key(key);
        let entry = keyring::Entry::new(service, account).map_err(err)?;
        entry.set_password(value).map_err(err)
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        let (service, account) = split_key(key);
        let entry = keyring::Entry::new(service, account).map_err(err)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(err(e)),
        }
    }
}

fn err(e: keyring::Error) -> String {
    format!("keyring: {e}")
}

/// In-memory [`SecretStore`] for tests — the ONLY store implementation tests
/// may exercise (CI has no Secret Service). `fail_writes` simulates an
/// unavailable/refusing backend so the "keyring write failed → keep
/// plaintext + warn" path is testable without any OS integration.
#[derive(Default)]
pub struct MockSecretStore {
    entries: Mutex<BTreeMap<String, String>>,
    /// When true, every `put` fails (get/delete keep working).
    pub fail_writes: bool,
}

impl MockSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every `put` fails — the degraded-write double.
    pub fn failing_writes() -> Self {
        Self {
            entries: Mutex::new(BTreeMap::new()),
            fail_writes: true,
        }
    }

    /// Test assertion helper: does an entry exist under `key`?
    pub fn contains(&self, key: &str) -> bool {
        self.entries.lock().expect("mock store lock").contains_key(key)
    }

    /// Test assertion helper: the stored value under `key`.
    pub fn value(&self, key: &str) -> Option<String> {
        self.entries.lock().expect("mock store lock").get(key).cloned()
    }
}

impl SecretStore for MockSecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.entries.lock().expect("mock store lock").get(key).cloned())
    }

    fn put(&self, key: &str, value: &str) -> Result<(), String> {
        if self.fail_writes {
            return Err("mock keyring write failure".into());
        }
        self.entries
            .lock()
            .expect("mock store lock")
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.entries.lock().expect("mock store lock").remove(key);
        Ok(())
    }
}

/// Probe the real OS keyring: write → read → delete one probe entry. Any
/// failure means the backend is unusable (no Secret Service on the Linux
/// session, locked keychain, …) and the caller must fall back to plaintext.
fn probe_keyring() -> Result<KeyringStore, String> {
    let store = KeyringStore;
    const PROBE_VALUE: &str = "shannon-keyring-probe";
    store.put(PROBE_KEY, PROBE_VALUE)?;
    let read_back = store.get(PROBE_KEY)?;
    if read_back.as_deref() != Some(PROBE_VALUE) {
        return Err(format!(
            "keyring probe read-back mismatch: {read_back:?}"
        ));
    }
    store.delete(PROBE_KEY)?;
    Ok(store)
}

/// The resolved credential backend for this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialStorage {
    /// OS keyring probed working — secrets migrate out of the files.
    Keyring,
    /// Keyring unavailable — credentials stay in the F3 `0600` plaintext
    /// files. Never silent: the probe failure warns per domain and the UI
    /// pages show the degraded mode.
    FileFallback,
}

impl CredentialStorage {
    /// The wire token the UI's credential-storage line renders.
    pub fn as_wire(self) -> &'static str {
        match self {
            CredentialStorage::Keyring => "keyring",
            CredentialStorage::FileFallback => "plaintext_file",
        }
    }
}

/// Wire token for the UI's "credential storage" status line, given whether a
/// working keyring store is in play. `false` → the honest degraded token.
pub fn storage_mode(store_available: bool) -> &'static str {
    if store_available {
        "keyring"
    } else {
        "plaintext_file"
    }
}

static GLOBAL: OnceLock<Option<Arc<dyn SecretStore>>> = OnceLock::new();

/// Probe the OS keyring once and install the process-global store
/// (`None` = degraded plaintext fallback). Called exactly once from the
/// Tauri `setup` — never in tests, so tests never touch the real keyring.
///
/// On fallback, emits one structured [`tracing::warn!`] **per affected
/// domain** (A8: degradation must be visible, once per domain, with the
/// domain field). Returns the resolved backend for the caller's info log.
pub fn init_global() -> CredentialStorage {
    let resolved = init_from_probe(probe_keyring());
    let _ = GLOBAL.set(match resolved {
        CredentialStorage::Keyring => Some(Arc::new(KeyringStore) as Arc<dyn SecretStore>),
        CredentialStorage::FileFallback => None,
    });
    resolved
}

/// Pure decision core of [`init_global`] (unit-testable without touching the
/// real keyring): turn a probe result into the backend + the per-domain
/// visibility warns.
fn init_from_probe(probe: Result<KeyringStore, String>) -> CredentialStorage {
    match probe {
        Ok(_) => {
            tracing::info!(
                service = SERVICE,
                "OS keyring available — MCP OAuth tokens and data-source credentials will migrate into it"
            );
            CredentialStorage::Keyring
        }
        Err(e) => {
            // One warn per affected domain (A8: no silent degradation). The
            // gateway connections domain is untouched by this batch and
            // keeps its own error surfaces.
            for domain in ["mcp-oauth", "datasource"] {
                tracing::warn!(
                    domain,
                    error = %e,
                    "OS keyring unavailable — credentials for this domain stay in \
                     local files (owner-only 0600); the settings pages show the \
                     degraded credential-storage mode"
                );
            }
            CredentialStorage::FileFallback
        }
    }
}

/// The process-global store, `None` when the keyring probe failed (or before
/// startup init — i.e. in every unit test). Domain code treats `None` as
/// "plaintext fallback": read paths skip the keyring lookup, migration is a
/// no-op, and deletes simply have nothing to clean.
pub fn global() -> Option<Arc<dyn SecretStore>> {
    GLOBAL.get().and_then(|store| store.clone())
}

/// Best-effort delete of one MCP server's keyring entry (uninstall cleanup —
/// orphaned entries are a new leak surface). A missing entry is success; a
/// real failure warns (with the domain + name) but never blocks the
/// uninstall itself.
pub fn delete_mcp_oauth_secret(store: Option<&dyn SecretStore>, server: &str) {
    delete_secret(store, &mcp_oauth_key(server), "mcp-oauth", server);
}

/// Best-effort delete of one data source's keyring entry (uninstall cleanup).
pub fn delete_datasource_secret(store: Option<&dyn SecretStore>, slug: &str) {
    delete_secret(store, &datasource_key(slug), "datasource", slug);
}

fn delete_secret(store: Option<&dyn SecretStore>, key: &str, domain: &str, name: &str) {
    let Some(store) = store else {
        return; // degraded mode: nothing ever left the plaintext file
    };
    match store.delete(key) {
        Ok(()) => {}
        Err(e) => tracing::warn!(
            domain,
            name,
            key,
            error = %e,
            "failed to delete keyring entry on uninstall — the orphaned entry \
             should be removed manually"
        ),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Minimal warn-capturing subscriber for asserting the A8 visibility
    //! requirements ("degradation must warn") without new dependencies.
    //! Scope-local: `with_default` never installs a global subscriber, so it
    //! cannot interfere with parallel tests.

    use std::sync::{Arc, Mutex};

    /// Records the message of every `WARN` event raised inside the scope.
    #[derive(Default, Clone)]
    pub(crate) struct WarnCapture {
        warnings: Arc<Mutex<Vec<String>>>,
    }

    impl WarnCapture {
        pub(crate) fn warnings(&self) -> Vec<String> {
            self.warnings.lock().expect("warn capture lock").clone()
        }
    }

    struct MessageVisitor<'a> {
        message: &'a mut String,
    }

    impl tracing::field::Visit for MessageVisitor<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                *self.message = format!("{value:?}");
            }
        }
    }

    impl tracing::Subscriber for WarnCapture {
        fn enabled(&self, meta: &tracing::Metadata<'_>) -> bool {
            meta.level() == &tracing::Level::WARN
        }
        fn new_span(&self, _attrs: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            if event.metadata().level() != &tracing::Level::WARN {
                return;
            }
            let mut message = String::new();
            event.record(&mut MessageVisitor {
                message: &mut message,
            });            self.warnings
                .lock()
                .expect("warn capture lock")
                .push(message);
        }
        fn enter(&self, _span: &tracing::span::Id) {}
        fn exit(&self, _span: &tracing::span::Id) {}
    }

    /// Run `f` under a fresh [`WarnCapture`].
    pub(crate) fn capture_warnings<R>(f: impl FnOnce() -> R) -> (WarnCapture, R) {
        let capture = WarnCapture::default();
        let out = tracing::subscriber::with_default(capture.clone(), f);
        (capture, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── namespace ─────────────────────────────────────────────────────────

    /// A8/acceptance: the key namespace is `shannon/<domain>/<name>`, and it
    /// never collides with the gateway connections' existing
    /// `<platform>/<slot>` keys (platforms are all distinct from `shannon`;
    /// the connections default service is `shannon-gateway`).
    #[test]
    fn key_namespace_is_scoped_and_disjoint_from_connections() {
        assert_eq!(mcp_oauth_key("linear-oauth"), "shannon/mcp-oauth/linear-oauth");
        assert_eq!(datasource_key("imap-home"), "shannon/datasource/imap-home");

        // Split at the first '/' → service is exactly `shannon`.
        assert_eq!(split_key("shannon/mcp-oauth/linear-oauth"), ("shannon", "mcp-oauth/linear-oauth"));
        assert_eq!(split_key("shannon/datasource/imap-home"), ("shannon", "datasource/imap-home"));

        // The gateway tenants' service names (see PlatformsCard.tsx
        // SECRET_MODEL + commands_connections DEFAULT_SERVICE) never equal
        // ours, so neither side can ever read the other's entries.
        let gateway_services = [
            "slack",
            "telegram",
            "discord",
            "matrix",
            "whatsapp",
            "wecom",
            "feishu",
            "dingtalk",
            "shannon-gateway",
        ];
        assert!(!gateway_services.contains(&SERVICE));
    }

    /// Special characters in server/slug names survive the key construction
    /// — the account part carries them verbatim (the OS keyrings accept
    /// arbitrary account strings).
    #[test]
    fn key_namespace_tolerates_special_characters() {
        let weird = "My Server (prod)/v2 测试 & <friends>";
        assert_eq!(
            mcp_oauth_key(weird),
            format!("shannon/mcp-oauth/{weird}")
        );
        assert_eq!(
            split_key(&mcp_oauth_key(weird)),
            ("shannon", format!("mcp-oauth/{weird}").as_str())
        );
        let slug = "a.b-c_d e";
        assert_eq!(datasource_key(slug), format!("shannon/datasource/{slug}"));
    }

    // ── mock store semantics ──────────────────────────────────────────────

    #[test]
    fn mock_store_round_trips_and_deletes_idempotently() {
        let store = MockSecretStore::new();
        let key = mcp_oauth_key("srv");
        assert_eq!(store.get(&key).unwrap(), None);
        store.put(&key, "{\"access_token\":\"t\"}").unwrap();
        assert_eq!(store.get(&key).unwrap().as_deref(), Some("{\"access_token\":\"t\"}"));
        assert!(store.contains(&key));
        store.delete(&key).unwrap();
        assert!(!store.contains(&key));
        // Deleting a missing entry is success (idempotent uninstall path).
        store.delete(&key).unwrap();
    }

    #[test]
    fn mock_store_failing_writes_simulates_unavailable_backend() {
        let store = MockSecretStore::failing_writes();
        assert!(store.put("shannon/x/y", "v").is_err());
    }

    // ── degradation visibility (A8) ───────────────────────────────────────

    /// A failed probe must resolve to the plaintext fallback AND warn once
    /// per affected domain — never silently degrade.
    #[test]
    fn failed_probe_falls_back_with_one_warn_per_domain() {
        let (capture, resolved) = test_support::capture_warnings(|| {
            init_from_probe(Err("no secret service".into()))
        });
        assert_eq!(resolved, CredentialStorage::FileFallback);
        let warnings = capture.warnings();
        assert_eq!(warnings.len(), 2, "one warn per affected domain: {warnings:?}");
        assert!(warnings[0].contains("keyring unavailable"), "{warnings:?}");
        assert!(warnings[1].contains("keyring unavailable"), "{warnings:?}");
    }

    /// A working probe resolves to the keyring backend (no warn).
    #[test]
    fn working_probe_resolves_keyring_without_warn() {
        let (capture, resolved) = test_support::capture_warnings(|| init_from_probe(Ok(KeyringStore)));
        assert_eq!(resolved, CredentialStorage::Keyring);
        assert!(capture.warnings().is_empty());
    }

    /// The wire tokens the UI status lines render.
    #[test]
    fn storage_mode_wire_tokens() {
        assert_eq!(storage_mode(true), "keyring");
        assert_eq!(storage_mode(false), "plaintext_file");
        assert_eq!(CredentialStorage::Keyring.as_wire(), "keyring");
        assert_eq!(CredentialStorage::FileFallback.as_wire(), "plaintext_file");
    }

    // ── uninstall cleanup ─────────────────────────────────────────────────

    #[test]
    fn delete_helpers_are_noops_without_a_store_and_best_effort_with_one() {
        // Degraded mode: nothing in the keyring, delete must be a silent noop.
        delete_mcp_oauth_secret(None, "srv");
        delete_datasource_secret(None, "slug");

        // With a store: entries actually disappear.
        let store = MockSecretStore::new();
        store.put(&mcp_oauth_key("srv"), "v").unwrap();
        store.put(&datasource_key("slug"), "v").unwrap();
        delete_mcp_oauth_secret(Some(&store), "srv");
        delete_datasource_secret(Some(&store), "slug");
        assert!(!store.contains(&mcp_oauth_key("srv")));
        assert!(!store.contains(&datasource_key("slug")));

        // Deleting a missing entry stays quiet (no warn, no error).
        let (capture, ()) = test_support::capture_warnings(|| {
            delete_mcp_oauth_secret(Some(&store), "never-there");
        });
        assert!(capture.warnings().is_empty());
    }
}
