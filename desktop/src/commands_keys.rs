//! R4-3 (desktop slice) — per-provider **multi-key management** commands.
//!
//! The engine side landed with R4-3: a credential entry carries
//! `value` + `extra_values` where **slot 0 is the ACTIVE key** and the rest
//! are rotation spares (`CredentialManager::add_key` /
//! `remove_key` / `activate_key`); the resolver fans them out in rotation
//! order (`provider_resolver::resolve_credential_keys`) and the engine
//! walks down the list on auth failures / persistent 429s before any
//! provider failover. The CLI drives it via `shannon providers keys …`;
//! this module is the desktop's slice of the same store.
//!
//! Wire contract (mirrors the CLI's display contract):
//! - keys are listed as `{index, active, masked_hint}` rows in rotation
//!   order — the ACTIVE key is index 0 and **full key material never
//!   crosses the wire** (same 0600 store `/connect` writes, same
//!   `sk-pri…aaaa`-style masking the CLI prints);
//! - `add_provider_key` accepts PLAINTEXT — the same trust level as the
//!   desktop's own `/connect` equivalent (`save_provider` /
//!   `configure('api_key')`, which already hand the webview a plaintext
//!   field). It is stored via the engine `CredentialManager`, never
//!   persisted anywhere else;
//! - `remove_provider_key` / `activate_provider_key` map 1:1 onto the
//!   engine mutators (removing slot 0 promotes the next stored key; the
//!   LAST remaining key is refused — delete the credential by editing the
//!   provider instead).
//!
//! Hot-reload: `build_client_from_resolved` bakes `api_key` +
//! `alternate_api_keys` into the live `AppState::client_config`, so any
//! mutation against the **active** provider re-runs
//! `rebuild_client_config_from_store` (the same rebuild
//! `commands_config`'s provider/api_key arms perform) — the next send picks
//! the new rotation up without a restart. Mutations against a non-active
//! provider need no rebuild: their client config is built at activation
//! time from the store + credential files.
//!
//! First-key note: the engine resolver only reads the credential file when
//! the provider slot's `credential` reference is
//! [`shannon_types::provider_config::CredentialRef::Store`]. A provider
//! saved keyless carries `Ephemeral`; when the FIRST key is added to the
//! ACTIVE slot through this surface the reference is flipped to
//! `Store { service }` in `providers.toml` (the desktop's service
//! convention: the provider id — what `store_provider_key` writes). For a
//! non-active slot the flip is deliberately skipped: activation re-lands
//! the slot through `ProviderConnection::to_provider_profile`, which
//! recomputes `Store` from credential-file presence anyway, and the only
//! store-level upsert primitive would steal the active pointer.

use crate::commands::AppState;
use crate::events;
use crate::events::event_names;
use shannon_types::provider_config::{CredentialRef, ProviderModelConfig};

/// One row of the per-provider "API keys" list. `index` is the rotation
/// position (0 = ACTIVE), `masked_hint` is display-only and never carries
/// the full key.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderKeySummary {
    pub index: u32,
    pub active: bool,
    pub masked_hint: String,
}

/// Mask a key for display: never the full value. Long keys keep a
/// recognizable head/tail (`sk-pri…aaaa`); short ones collapse entirely.
/// Port of the CLI's `mask_key` (crates/shannon-cli/src/commands_providers.rs)
/// so both surfaces render the same string for the same key.
pub(crate) fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "…".to_string();
    }
    let head: String = chars.iter().take(6).collect();
    let tail: String = chars.iter().rev().take(4).rev().collect();
    format!("{head}…{tail}")
}

/// Project raw rotation-ordered keys to wire rows. Row 0 is the ACTIVE key
/// (the engine's storage invariant: `value` first, then `extra_values`).
pub(crate) fn summarize_keys(keys: &[String]) -> Vec<ProviderKeySummary> {
    keys.iter()
        .enumerate()
        .map(|(i, key)| ProviderKeySummary {
            index: i as u32,
            active: i == 0,
            masked_hint: mask_key(key),
        })
        .collect()
}

/// Where a provider's keys live, resolved from the engine store.
#[derive(Debug, Clone)]
pub(crate) struct KeyTarget {
    /// The credential-store service name. Desktop convention: the provider
    /// id (what `commands_config::store_provider_key` writes); a slot that
    /// references another store service (CLI-authored) wins over the
    /// convention.
    pub service: String,
    /// True when the slot's credential reference is not `Store` yet and the
    /// engine would never read the key file — the first-key flip below has
    /// to rewrite it.
    pub needs_flip: bool,
    /// True when the slot is the active model profile's active target — the
    /// case where a mutation must hot-reload the running client config.
    pub is_active: bool,
    /// The slot's current `active_target.model_id` (preserved by the flip's
    /// re-upsert).
    pub active_model_id: String,
}

/// Resolve the credential service for `provider_id` from a committed store
/// snapshot. Unknown providers are an error; a slot without a `Store`
/// credential falls back to the desktop's `service = provider id`
/// convention (and reports `needs_flip`).
pub(crate) fn resolve_key_target(
    config: &ProviderModelConfig,
    provider_id: &str,
) -> Result<KeyTarget, String> {
    let id = provider_id.trim();
    let model_profile = config
        .active_model_profile()
        .ok_or_else(|| "no active model profile in providers.toml".to_string())?;
    let slot = model_profile
        .providers
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| {
            format!("provider '{id}' is not configured — add one in Settings → Models first")
        })?;
    let is_store = matches!(slot.credential, CredentialRef::Store { .. });
    let service = match &slot.credential {
        CredentialRef::Store { service } => service.clone(),
        // Desktop convention — `store_provider_key` writes key files under
        // the provider id, so the flip below points the slot at that name.
        _ => id.to_string(),
    };
    Ok(KeyTarget {
        service,
        needs_flip: !is_store,
        is_active: model_profile.active_target.provider_id == id,
        active_model_id: model_profile.active_target.model_id.clone(),
    })
}

/// Flip a slot's credential reference to `Store { service }` in place.
/// No-op (returns false) when the slot already carries a `Store`
/// reference. Pure so the command can drive it under the store lock and
/// tests can pin it without one.
pub(crate) fn flip_credential_to_store(
    slot: &mut shannon_types::provider_config::ProviderProfile,
    service: &str,
) -> bool {
    if matches!(slot.credential, CredentialRef::Store { .. }) {
        return false;
    }
    slot.credential = CredentialRef::Store {
        service: service.to_string(),
    };
    true
}

/// Snapshot the committed store config under the short-lived read lock
/// (ADR-0009 snapshot-then-release, same shape as
/// `ProviderReadSnapshot::capture`).
async fn snapshot_config(state: &tauri::State<'_, AppState>) -> ProviderModelConfig {
    let store = state.provider_store.lock().await;
    store.config().clone()
}

/// List a provider's stored keys in rotation order. A provider with no
/// credential entry yet lists as EMPTY (the UI offers the add form) — only
/// a malformed store or an unknown provider errors.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_provider_keys(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<ProviderKeySummary>, String> {
    list_provider_keys_body(&state, &provider_id).await
}

pub(crate) async fn list_provider_keys_body(
    state: &tauri::State<'_, AppState>,
    provider_id: &str,
) -> Result<Vec<ProviderKeySummary>, String> {
    let config = snapshot_config(state).await;
    let target = resolve_key_target(&config, provider_id)?;
    // Pure disk read — the exact function the engine resolver uses at
    // request time, so the list can never drift from what rotation walks.
    let keys = shannon_core::credential_manager::read_credential_keys_default(&target.service)
        .unwrap_or_default();
    Ok(summarize_keys(&keys))
}

/// Humanize credential-store errors for the keys surface (the CLI's
/// `map_key_error`, minus the anyhow layer). A missing entry gets the
/// "connect first" hint; everything else prints as-is.
fn map_key_error(e: shannon_core::credential_manager::CredentialError, service: &str) -> String {
    use shannon_core::credential_manager::CredentialError;
    match e {
        CredentialError::NotFound(_) => format!(
            "no credential stored for service '{service}' — set the provider's API key first"
        ),
        other => other.to_string(),
    }
}

/// Open the credential store synced with disk (the CLI's `run_providers_keys`
/// preamble: `with_dir`/`new` start from an EMPTY in-memory cache, so a
/// mutation without `load()` would not see the stored entry).
fn open_credential_manager() -> Result<shannon_core::credential_manager::CredentialManager, String>
{
    use shannon_core::credential_manager::CredentialManager;
    let mut manager =
        CredentialManager::new().map_err(|e| format!("could not open credential store: {e}"))?;
    manager
        .load()
        .map_err(|e| format!("could not read credential store: {e}"))?;
    Ok(manager)
}

/// Shared mutation spine for add/remove/activate: resolve the target, run
/// the (active-slot) first-key flip, apply the credential mutation, then
/// hot-reload the client config when the mutated provider is the active
/// one. Returns the fresh key list so the UI never needs a second round
/// trip.
async fn mutate_provider_keys<F>(
    state: &tauri::State<'_, AppState>,
    app_handle: &tauri::AppHandle,
    provider_id: &str,
    event_detail: String,
    mutate: F,
) -> Result<Vec<ProviderKeySummary>, String>
where
    F: FnOnce(
        &mut shannon_core::credential_manager::CredentialManager,
        &str,
    ) -> Result<(), shannon_core::credential_manager::CredentialError>,
{
    use tauri::Emitter;

    let config = snapshot_config(state).await;
    let target = resolve_key_target(&config, provider_id)?;

    // First-key flip (ACTIVE slot only — see the module docs for why the
    // non-active case is deliberately deferred to activation time). The
    // re-upsert re-points `active_target` at this slot, which is a no-op
    // precisely because the slot IS the target; the model id is preserved.
    if target.needs_flip && target.is_active {
        crate::commands_profiles::with_model_profile_store(state, |store| {
            let model_profile = store
                .config()
                .active_model_profile()
                .ok_or_else(|| "no active model profile in providers.toml".to_string())?;
            let mut slot = model_profile
                .providers
                .iter()
                .find(|p| p.id == provider_id.trim())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "provider '{}' is no longer in the store",
                        provider_id.trim()
                    )
                })?;
            if flip_credential_to_store(&mut slot, &target.service) {
                store.upsert_profile(slot, &target.active_model_id);
            }
            Ok(())
        })
        .await?;
    }

    {
        let mut manager = open_credential_manager()?;
        mutate(&mut manager, &target.service).map_err(|e| map_key_error(e, &target.service))?;
    }

    // Hot-reload: the running client embeds `api_key` +
    // `alternate_api_keys`, so an active-provider mutation must rebuild for
    // the next send to see it. Never blocks on rebuild failure (the store
    // and credential file are already committed — same contract as the
    // profile switch).
    if target.is_active {
        if let Err(e) = crate::commands_config::rebuild_client_config_from_store(state).await {
            tracing::warn!("provider key mutation: client config rebuild failed: {e}");
        }
    }

    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "provider_api_keys".into(),
            value: event_detail,
        },
    );
    list_provider_keys_body(state, provider_id).await
}

/// Add a key to a provider's rotation list (plaintext — the same trust
/// level as the desktop's `/connect` equivalent, which also hands the
/// webview a plaintext field). Duplicates and empty values are refused by
/// the engine with the offending slot named. Returns the fresh list.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn add_provider_key(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    provider_id: String,
    key: String,
) -> Result<Vec<ProviderKeySummary>, String> {
    if key.trim().is_empty() {
        return Err("cannot add an empty key".to_string());
    }
    mutate_provider_keys(&state, &app_handle, &provider_id, provider_id.clone(), {
        let key = key;
        move |manager, service| manager.add_key(service, key.trim()).map(|_| ())
    })
    .await
}

/// Remove the key at `index` from a provider's rotation list. Removing the
/// ACTIVE key (index 0) promotes the next stored key automatically; the
/// LAST remaining key is refused by the engine (a credential entry always
/// holds at least one — replace it by editing the provider instead).
/// Returns the fresh list.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn remove_provider_key(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    provider_id: String,
    index: u32,
) -> Result<Vec<ProviderKeySummary>, String> {
    mutate_provider_keys(&state, &app_handle, &provider_id, provider_id.clone(), {
        move |manager, service| manager.remove_key(service, index as usize).map(|_| ())
    })
    .await
}

/// Make the key at `index` the provider's ACTIVE key (engine swap-to-slot-0
/// semantics; `activate(0)` is a accepted no-op). The running client is
/// hot-reloaded when this provider is active, so the swap takes effect on
/// the next send. Returns the fresh list.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn activate_provider_key(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    provider_id: String,
    index: u32,
) -> Result<Vec<ProviderKeySummary>, String> {
    mutate_provider_keys(&state, &app_handle, &provider_id, provider_id.clone(), {
        move |manager, service| manager.activate_key(service, index as usize).map(|_| ())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_core::credential_manager::CredentialManager;
    use shannon_core::provider_config_store::ProviderConfigStore;
    use shannon_types::provider_config::{
        ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind,
        ProviderModelConfig, ProviderProfile, Scope,
    };
    use std::collections::HashMap;

    fn slot(id: &str, credential: CredentialRef) -> ProviderProfile {
        ProviderProfile {
            id: id.to_string(),
            kind: ProviderKind::Anthropic,
            display_name: format!("{id} label"),
            base_url: "https://api.anthropic.com".to_string(),
            models_url: None,
            credential,
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: Default::default(),
            models: Vec::new(),
        }
    }

    fn store_with_roster() -> ProviderConfigStore {
        let mut profile = ModelProfile {
            name: "default".to_string(),
            active_target: ActiveTarget {
                provider_id: "anthropic-main".to_string(),
                model_id: "claude-sonnet-4-6".to_string(),
                scope: Scope::Global,
            },
            providers: vec![
                slot(
                    "anthropic-main",
                    CredentialRef::Store {
                        service: "anthropic-main".to_string(),
                    },
                ),
                slot("glm", CredentialRef::Ephemeral),
            ],
            auxiliary: HashMap::new(),
            credential_scope: CredentialScope::Shared,
        };
        profile.providers[1].kind = ProviderKind::OpenAiCompatible;
        let mut config = ProviderModelConfig::default();
        config.profiles.insert("default".to_string(), profile);
        ProviderConfigStore::from_config(config)
    }

    // ── masking (the never-leak-a-key contract) ──────────────────────────

    #[test]
    fn mask_key_never_shows_the_full_value() {
        let long = "sk-ant-api03-0123456789abcdefABCDEFGHIJ";
        let masked = mask_key(long);
        assert_eq!(masked, "sk-ant…GHIJ", "head 6 + tail 4");
        assert!(!masked.contains(long), "full key must never leak");
        assert!(masked.chars().count() < long.chars().count());

        // Short keys collapse entirely — a 6-char key has no safe head/tail.
        assert_eq!(mask_key("short"), "…");
        assert_eq!(mask_key("12345678"), "…", "exactly-8 still collapses");
        assert_eq!(mask_key(""), "…");
    }

    #[test]
    fn summarize_keys_marks_only_slot_zero_active_and_masks_every_row() {
        let keys: Vec<String> = vec![
            "sk-ant-api03-0123456789abcdef".to_string(),
            "sk-rotation-spare-99887766554433221100".to_string(),
            "tiny".to_string(),
        ];
        let rows = summarize_keys(&keys);
        assert_eq!(rows.len(), 3);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row.index, i as u32);
            assert_eq!(row.active, i == 0, "only the first row is active");
            assert!(!row.masked_hint.contains(keys[i].as_str()));
        }
        assert_eq!(rows[2].masked_hint, "…", "short keys collapse");
    }

    // ── service resolution + the first-key flip ──────────────────────────

    #[test]
    fn resolve_key_target_maps_store_service_and_flags_ephemeral() {
        let store = store_with_roster();
        let config = store.config();

        // Store-backed slot → the referenced service, no flip, active.
        let active = resolve_key_target(config, "anthropic-main").unwrap();
        assert_eq!(active.service, "anthropic-main");
        assert!(!active.needs_flip);
        assert!(active.is_active);
        assert_eq!(active.active_model_id, "claude-sonnet-4-6");

        // Ephemeral slot → the desktop convention (service = provider id)
        // plus the flip flag; NOT active → no client hot-reload expected.
        let spare = resolve_key_target(config, "glm").unwrap();
        assert_eq!(spare.service, "glm");
        assert!(spare.needs_flip);
        assert!(!spare.is_active);

        // Unknown provider → a clean, UI-displayable error.
        let err = resolve_key_target(config, "ghost").unwrap_err();
        assert!(err.contains("not configured"), "got: {err}");
    }

    #[test]
    fn resolve_key_target_respects_a_referenced_service_over_the_convention() {
        // CLI-authored slot pointing at a differently-named store service —
        // the referenced name wins so the CLI and desktop manage the SAME
        // rotation list.
        let mut profile = ModelProfile {
            name: "default".to_string(),
            active_target: ActiveTarget {
                provider_id: String::new(),
                model_id: String::new(),
                scope: Scope::Global,
            },
            providers: vec![slot(
                "glm",
                CredentialRef::Store {
                    service: "zhipu-work".to_string(),
                },
            )],
            auxiliary: HashMap::new(),
            credential_scope: CredentialScope::Shared,
        };
        profile.providers[0].kind = ProviderKind::OpenAiCompatible;
        let mut config = ProviderModelConfig::default();
        config.profiles.insert("default".to_string(), profile);
        let target = resolve_key_target(&config, "glm").unwrap();
        assert_eq!(target.service, "zhipu-work");
        assert!(!target.needs_flip);
    }

    #[test]
    fn flip_credential_to_store_flips_once_and_only_once() {
        let mut ephemeral = slot("glm", CredentialRef::Ephemeral);
        assert!(flip_credential_to_store(&mut ephemeral, "glm"));
        assert_eq!(
            ephemeral.credential,
            CredentialRef::Store {
                service: "glm".to_string()
            }
        );
        // Second call is a no-op — a Store ref is never rewritten (a
        // CLI-authored `store:zhipu-work` reference must survive).
        assert!(!flip_credential_to_store(&mut ephemeral, "other"));
        assert_eq!(
            ephemeral.credential,
            CredentialRef::Store {
                service: "glm".to_string()
            }
        );
    }

    #[test]
    fn upsert_based_flip_repoints_the_target_so_only_active_slots_may_flip() {
        // The command's flip drives `upsert_profile` (the only pub store
        // mutator that can rewrite one slot), which re-points the active
        // target at the rewritten slot. This pins that behavior — the
        // documented reason the command only flips ACTIVE slots (where the
        // repoint is a no-op): a non-active flip via this primitive would
        // steal the pointer, and the engine has no targeted slot rewriter.
        let mut store = store_with_roster();
        {
            let model_profile = store.config().active_model_profile().unwrap();
            let mut glm_slot = model_profile.providers[1].clone();
            assert!(flip_credential_to_store(&mut glm_slot, "glm"));
            store.upsert_profile(glm_slot, "claude-sonnet-4-6");
        }
        let config = store.config();
        let model_profile = config.active_model_profile().unwrap();
        // Flip happened…
        assert!(matches!(
            model_profile.providers[1].credential,
            CredentialRef::Store { .. }
        ));
        // …and the pointer followed the upsert (pinning the constraint).
        assert_eq!(model_profile.active_target.provider_id, "glm");
        assert_eq!(model_profile.providers.len(), 2, "no sibling dropped");
    }

    // ── engine mutator round trip (the exact calls the commands make) ────

    #[tokio::test]
    async fn add_activate_remove_round_trip_through_a_hermetic_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut mgr = CredentialManager::with_dir(tmp.path().join("credentials")).unwrap();
        mgr.store(shannon_core::credential_manager::Credential::new(
            "anthropic-main label",
            "anthropic-main",
            "sk-key-one-aaaaaaaaaaaaaaaa",
        ))
        .unwrap();

        assert_eq!(
            mgr.add_key("anthropic-main", "sk-key-two-bbbbbbbbbbbb")
                .unwrap(),
            2
        );
        // Duplicates are refused with the existing slot named.
        let dup = mgr
            .add_key("anthropic-main", "sk-key-one-aaaaaaaaaaaaaaaa")
            .unwrap_err();
        assert!(dup.to_string().contains("slot 0"), "got: {dup}");
        assert!(
            mgr.add_key("anthropic-main", "   ").is_err(),
            "blank refused"
        );

        // Rotation order: active first.
        assert_eq!(
            mgr.keys("anthropic-main").unwrap(),
            vec!["sk-key-one-aaaaaaaaaaaaaaaa", "sk-key-two-bbbbbbbbbbbb"]
        );

        // Activate slot 1 → swap to slot 0.
        mgr.activate_key("anthropic-main", 1).unwrap();
        assert_eq!(
            mgr.keys("anthropic-main").unwrap()[0],
            "sk-key-two-bbbbbbbbbbbb"
        );
        let oob = mgr.activate_key("anthropic-main", 9).unwrap_err();
        assert!(oob.to_string().contains("out of range"), "got: {oob}");

        // Removing the ACTIVE key promotes the next one; removing the LAST
        // remaining key is refused.
        mgr.remove_key("anthropic-main", 0).unwrap();
        assert_eq!(
            mgr.keys("anthropic-main").unwrap(),
            vec!["sk-key-one-aaaaaaaaaaaaaaaa"]
        );
        let last = mgr.remove_key("anthropic-main", 0).unwrap_err();
        assert!(
            last.to_string().contains("last remaining key"),
            "got: {last}"
        );

        // The list command's read path sees the same rotation (disk round
        // trip through the per-service read the engine resolver uses — the
        // hermetic-dir variant, since `read_credential_keys_default` would
        // read the ambient `~/.shannon/credentials`).
        drop(mgr);
        let hermetic = shannon_core::credential_manager::read_credential_keys(
            &tmp.path().join("credentials"),
            "anthropic-main",
        )
        .unwrap();
        assert_eq!(hermetic, vec!["sk-key-one-aaaaaaaaaaaaaaaa"]);

        // A missing entry reads as None → the command's empty-list contract.
        assert!(
            shannon_core::credential_manager::read_credential_keys(
                &tmp.path().join("credentials"),
                "ghost"
            )
            .is_none()
        );
    }
}
