//! R3-2 (desktop slice) — provider **model profile** commands.
//!
//! providers.toml v2 carries a named `profiles` map (`ModelProfile`:
//! provider roster + active target + credential scope) plus an
//! `active_profile` pointer (empty ⇒ `"default"`). The engine's
//! multi-profile support and its listing surface are landing
//! incrementally on the `ProviderConfigService` side; the desktop ships
//! its minimal surface NOW against the store directly — the same
//! write-path pattern `commands_config` uses for managed provider
//! connections (`with_engine_store` semantics: in-process mutex first,
//! cross-process flock second, reload → mutate → persist inside the
//! critical section, unconditional restore into the guard).
//!
//! Scope (R5 batch): list (name, provider count, active marker), switch
//! (the UI confirms when the target profile is empty), create, and — the
//! R3-2 deferred slice — rename + delete. The store mutators
//! (`rename_model_profile` / `remove_model_profile(name, force)`) existed
//! all along; the desktop commands now drive them with the same UI
//! contract: rename surfaces engine duplicates/not-found inline, delete
//! passes `force = true` (the UI's ConfirmDialog is the consent) and
//! reports which profile the engine's active-pointer fallback picked.
//!
//! Switching is a pure pointer move in `providers.toml`
//! (`set_active_profile_key`) + a `rebuild_client_config_from_store`
//! re-point of the global default, so every read path that already
//! resolves through `resolve_active_target` (engine launch, `get_status`,
//! `send_message`'s global config) follows the switch with no per-command
//! special cases.

use crate::commands::AppState;
use crate::events;
use crate::events::event_names;
use shannon_core::provider_config_store::ProviderConfigStore;

/// One row of the Settings → Models "Profiles" list.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderProfileSummary {
    /// The `profiles` map key (`"default"` or a validated name).
    pub name: String,
    /// Provider slots in the profile (0 for a freshly created one — the UI
    /// asks for confirmation before switching to an empty profile).
    pub provider_count: u32,
    /// True when this profile is the engine's active one
    /// (`active_profile_key()`).
    pub active: bool,
    /// The profile's `active_target.model_id` when set — display-only.
    pub model: Option<String>,
}

/// Summarize a committed store snapshot. `"default"` is pinned first and
/// the rest alphabetical (`profile_names()` order), so the UI list is
/// stable regardless of `HashMap` iteration order.
pub(crate) fn summarize_profiles(
    config: &shannon_types::provider_config::ProviderModelConfig,
) -> Vec<ProviderProfileSummary> {
    let active_key = config.active_profile_key();
    let mut names = config.profile_names();
    // Stable UI order: default first, rest alphabetical.
    names.sort_by(|a, b| {
        let a_default = a == shannon_types::provider_config::ProviderModelConfig::DEFAULT_PROFILE;
        let b_default = b == shannon_types::provider_config::ProviderModelConfig::DEFAULT_PROFILE;
        b_default.cmp(&a_default).then_with(|| a.cmp(b))
    });
    names
        .into_iter()
        .filter_map(|name| {
            let profile = config.profiles.get(&name)?;
            let model = profile.active_target.model_id.trim();
            Some(ProviderProfileSummary {
                active: name == active_key,
                name,
                provider_count: profile.providers.len() as u32,
                model: (!model.is_empty()).then(|| model.to_string()),
            })
        })
        .collect()
}

/// R3-2 store-level R-M-W helper — the model-profile sibling of
/// `commands_config::with_engine_store`. Lock ordering and restore
/// semantics are identical (in-process `provider_store` mutex first, then
/// the cross-process flock via `acquire_exclusive_lock`; reload the
/// committed state inside the critical section; unconditional restore even
/// on `Err` so a failed write can't leave the in-process snapshot empty).
///
/// It exists because the profile mutators (`insert_model_profile`,
/// `set_active_profile_key`) live on the **store** while the
/// `ProviderConfigService` locked convenience methods don't expose them
/// yet — the desktop needs raw store access under the same flock contract
/// (`ProviderConfigStore::save_locked` persists without re-locking, per
/// the reentrancy contract on `acquire_exclusive_lock`). When the
/// service-level profile APIs land, this helper should collapse into
/// `with_engine_store`.
///
/// R5: shared with `commands_keys`, which needs the same raw-store
/// critical section for the R4-3 first-key credential-ref flip.
pub(crate) async fn with_model_profile_store<R, F>(
    state: &tauri::State<'_, AppState>,
    f: F,
) -> Result<R, String>
where
    F: FnOnce(&mut ProviderConfigStore) -> Result<R, String>,
{
    use shannon_core::provider_config_store::{acquire_exclusive_lock, default_path};

    let mut guard = state.provider_store.lock().await;
    let mut store = std::mem::take(&mut *guard);
    let outcome = (|store: &mut ProviderConfigStore| -> Result<R, String> {
        let path = store
            .last_path()
            .map(|p| p.to_path_buf())
            .or_else(default_path)
            .ok_or_else(|| "could not resolve the providers.toml path".to_string())?;
        let flock = acquire_exclusive_lock(&path)
            .map_err(|e| format!("could not lock providers.toml: {e}"))?;
        store
            .reload_locked()
            .map_err(|e| format!("could not reload providers.toml: {e}"))?;
        let result = f(store);
        if result.is_ok() {
            store
                .save_locked()
                .map_err(|e| format!("could not persist providers.toml: {e}"))?;
        }
        drop(flock);
        result
    })(&mut store);
    // Unconditional restore (see `with_engine_store`).
    *guard = store;
    outcome
}

/// List the engine store's model profiles with their active marker.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_provider_profiles(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderProfileSummary>, String> {
    list_provider_profiles_body(&state).await
}

pub(crate) async fn list_provider_profiles_body(
    state: &tauri::State<'_, AppState>,
) -> Result<Vec<ProviderProfileSummary>, String> {
    let config = {
        let store = state.provider_store.lock().await;
        store.config().clone()
    };
    Ok(summarize_profiles(&config))
}

/// Create an empty named model profile (name prompt flow). The profile is
/// created INACTIVE — activation is an explicit switch, so a stray click
/// can never re-point the global default at a provider-less profile.
///
/// The name is validated by the engine's shared
/// `validate_profile_name` (the same contract `/profiles new` enforces),
/// duplicates are rejected, and the fresh list is returned so the UI can
/// render the new row without a second round trip.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn create_provider_profile(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    name: String,
) -> Result<Vec<ProviderProfileSummary>, String> {
    use tauri::Emitter;

    let name = shannon_types::provider_config::validate_profile_name(&name)
        .map_err(|e| format!("create_provider_profile: {e}"))?;

    with_model_profile_store(&state, |store| {
        store.insert_model_profile(&name).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                format!("A profile named '{name}' already exists")
            } else {
                format!("could not create profile '{name}': {e}")
            }
        })?;
        Ok(())
    })
    .await?;

    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "provider_profiles".into(),
            value: name.clone(),
        },
    );
    list_provider_profiles_body(&state).await
}

/// Switch the engine's active model profile. The caller (UI) confirms
/// first when the target profile has no provider slots — the desktop's
/// global default degrades to the empty default client config in that
/// case, which the confirm dialog says out loud.
///
/// Persists the pointer in `providers.toml` (`active_profile`) and
/// rebuilds `AppState::client_config` so every query path follows the
/// switch on the next send.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn set_active_provider_profile(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    name: String,
) -> Result<Vec<ProviderProfileSummary>, String> {
    use tauri::Emitter;

    let name = shannon_types::provider_config::validate_profile_name(&name)
        .map_err(|e| format!("set_active_provider_profile: {e}"))?;

    with_model_profile_store(&state, |store| {
        if !store.config().profiles.contains_key(&name) {
            return Err(format!(
                "profile '{name}' not found; available profiles: {}",
                store.model_profile_names().join(", ")
            ));
        }
        store.set_active_profile_key(&name);
        Ok(())
    })
    .await?;

    // Re-point the global default. Resolution flows through
    // `resolve_active_target` → `active_profile`, so this single rebuild is
    // the whole switch; an empty target profile lands on the default client
    // config (`unwrap_or_default`), which the UI's confirm dialog warned
    // about. Never blocks on resolution failure.
    if let Err(e) = crate::commands_config::rebuild_client_config_from_store(&state).await {
        tracing::warn!("profile switch: client config rebuild failed: {e}");
    }

    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "active_model_profile".into(),
            value: name,
        },
    );
    list_provider_profiles_body(&state).await
}

/// Rename a model profile (R3-2 deferred slice). The engine mutator moves
/// the `profiles` map entry, rewrites the profile's own `name` field, and
/// **follows the active pointer** when the renamed profile was the active
/// one — a rename never changes which profile is live. Duplicates and
/// unknown source names are engine errors, surfaced to the UI verbatim
/// (the inline form validates the same contract client-side first).
///
/// When the renamed profile was active, the global default is re-resolved
/// (a content-preserving rebuild — the pointer now carries the new name)
/// and the `active_model_profile` event re-announces it so open windows
/// refresh their profile label.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn rename_provider_profile(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    old: String,
    new: String,
) -> Result<Vec<ProviderProfileSummary>, String> {
    use tauri::Emitter;

    let old = shannon_types::provider_config::validate_profile_name(&old)
        .map_err(|e| format!("rename_provider_profile: {e}"))?;
    let new = shannon_types::provider_config::validate_profile_name(&new)
        .map_err(|e| format!("rename_provider_profile: {e}"))?;
    if old == new {
        // Content-preserving no-op — cheaper than an engine round trip and
        // it keeps the UI's success path honest ("renamed" to itself).
        return list_provider_profiles_body(&state).await;
    }

    let was_active = with_model_profile_store(&state, |store| {
        let was_active = store.config().active_profile_key() == old;
        store.rename_model_profile(&old, &new).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                format!("A profile named '{new}' already exists")
            } else {
                format!("could not rename profile '{old}': {e}")
            }
        })?;
        Ok(was_active)
    })
    .await?;

    if was_active {
        // The active target is unchanged in content, but every surface that
        // displays the profile NAME must follow the pointer. Never blocks on
        // resolution failure (same contract as the switch command).
        if let Err(e) = crate::commands_config::rebuild_client_config_from_store(&state).await {
            tracing::warn!("profile rename: client config rebuild failed: {e}");
        }
        let _ = app_handle.emit(
            event_names::CONFIG_UPDATED,
            events::ConfigUpdatedPayload {
                key: "active_model_profile".into(),
                value: new.clone(),
            },
        );
    }
    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "provider_profiles".into(),
            value: new,
        },
    );
    list_provider_profiles_body(&state).await
}

/// Delete a model profile (R3-2 deferred slice). `force = true` is the
/// desktop's standing posture — the UI's ConfirmDialog is the consent, and
/// the dialog names the fallback when the target is the ACTIVE profile —
/// so the command never refuses on the engine's "refusing to delete the
/// active profile" guard. The parameter stays on the wire for symmetry
/// with the store/CLI contract.
///
/// Deleting the ACTIVE profile moves the pointer per the engine's
/// fallback: `"default"` when it survives, else the first remaining
/// profile alphabetically, else the pointer clears (resolution degrades
/// to synthesis). The fallback name is returned in
/// [`DeleteProfileOutcome::became_active`] so the UI can say out loud
/// which profile took over, and the client config is rebuilt (a real
/// target change this time).
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeleteProfileOutcome {
    /// The fresh profile list, exactly like the other profile commands —
    /// the active marker already reflects the engine's fallback.
    pub profiles: Vec<ProviderProfileSummary>,
    /// `Some(name)` when the deleted profile was the active one and the
    /// pointer moved to the engine's fallback; `None` when the deleted
    /// profile was not active (or the pointer cleared — an empty store).
    pub became_active: Option<String>,
}

#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn delete_provider_profile(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    name: String,
    force: Option<bool>,
) -> Result<DeleteProfileOutcome, String> {
    use tauri::Emitter;

    let name = shannon_types::provider_config::validate_profile_name(&name)
        .map_err(|e| format!("delete_provider_profile: {e}"))?;
    let force = force.unwrap_or(true);

    let became_active = with_model_profile_store(&state, |store| {
        store.remove_model_profile(&name, force).map_err(|e| {
            // The engine's two `InvalidData` refusals (last-remaining
            // profile; active-profile-without-force) read fine verbatim.
            format!("could not delete profile '{name}': {e}")
        })
    })
    .await?;

    if became_active.is_some() {
        // The active profile changed — re-point the global default (same
        // rebuild the switch command performs) and announce the takeover.
        if let Err(e) = crate::commands_config::rebuild_client_config_from_store(&state).await {
            tracing::warn!("profile delete: client config rebuild failed: {e}");
        }
        let _ = app_handle.emit(
            event_names::CONFIG_UPDATED,
            events::ConfigUpdatedPayload {
                key: "active_model_profile".into(),
                value: became_active.clone().unwrap_or_default(),
            },
        );
    }
    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "provider_profiles".into(),
            value: name,
        },
    );
    Ok(DeleteProfileOutcome {
        profiles: list_provider_profiles_body(&state).await?,
        became_active,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_types::provider_config::{
        ActiveTarget, CredentialScope, ModelProfile, ProviderModelConfig, Scope,
    };
    use std::collections::HashMap;

    fn empty_profile(name: &str) -> ModelProfile {
        ModelProfile {
            name: name.to_string(),
            active_target: ActiveTarget {
                provider_id: String::new(),
                model_id: String::new(),
                scope: Scope::Global,
            },
            providers: Vec::new(),
            auxiliary: HashMap::new(),
            credential_scope: CredentialScope::Shared,
        }
    }

    #[test]
    fn summarize_orders_default_first_and_marks_active() {
        let mut profiles = HashMap::new();
        profiles.insert("default".to_string(), empty_profile("default"));
        profiles.insert("work".to_string(), empty_profile("work"));
        let mut config = ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            active_profile: "work".to_string(),
            profiles,
            gateway: Default::default(),
        };
        // Give "work" some shape to assert on.
        config
            .profiles
            .get_mut("work")
            .unwrap()
            .active_target
            .model_id = "glm-5.3-flash".into();

        let rows = summarize_profiles(&config);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["default", "work"],
            "default pinned first, rest alphabetical"
        );

        let work = rows.iter().find(|r| r.name == "work").unwrap();
        assert!(work.active, "active_profile pointer marks the row");
        assert_eq!(work.model.as_deref(), Some("glm-5.3-flash"));
        assert_eq!(work.provider_count, 0);

        let default = rows.iter().find(|r| r.name == "default").unwrap();
        assert!(!default.active);
        assert_eq!(default.model, None, "empty active_target model stays None");
    }

    #[test]
    fn summarize_empty_store_is_empty_and_default_key_still_resolves() {
        let config = ProviderModelConfig::default();
        assert!(summarize_profiles(&config).is_empty());
        // An unset pointer still names "default" — the marker logic must
        // never panic on a fresh config.
        assert_eq!(config.active_profile_key(), "default");
    }

    #[tokio::test]
    async fn create_switch_and_list_round_trip_through_the_store() {
        use crate::commands::AppState;

        let state = AppState::new();
        // Re-point the store at a scratch file so the test never touches the
        // ambient ~/.shannon/providers.toml.
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("providers.toml");
        {
            let mut store = state.provider_store.lock().await;
            *store = ProviderConfigStore::load_or_default_at(&path);
        }

        // Seed a "default" profile with one provider so the store is in the
        // shape a configured desktop has (insert + switch, the same mutators
        // the commands drive).
        {
            let mut store = state.provider_store.lock().await;
            store.insert_model_profile("default").unwrap();
        }

        // Fresh store snapshot: default exists and is active.
        let config = {
            let store = state.provider_store.lock().await;
            store.config().clone()
        };
        let rows = summarize_profiles(&config);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "default");
        assert!(rows[0].active);

        // Create "work" (still inactive), then switch the pointer, persisting
        // exactly like the command layer does inside its critical section.
        {
            let mut store = state.provider_store.lock().await;
            store.insert_model_profile("work").unwrap();
            assert!(store.config().profiles.contains_key("work"));
            store.set_active_profile_key("work");
            store.save().expect("persist providers.toml");
        }
        let config = {
            let store = state.provider_store.lock().await;
            store.config().clone()
        };
        assert_eq!(
            config.active_profile_key(),
            "work",
            "switch persists the pointer"
        );
        let rows = summarize_profiles(&config);
        let work = rows.iter().find(|r| r.name == "work").unwrap();
        assert!(work.active);
        assert_eq!(work.provider_count, 0, "fresh profile is empty");

        // The round trip survives a reload from disk (save() above is the
        // same persistence `with_model_profile_store` performs via
        // `save_locked` inside its flock).
        let reloaded = ProviderConfigStore::load_or_default_at(&path);
        assert_eq!(reloaded.config().active_profile_key(), "work");
        assert!(reloaded.config().profiles.contains_key("default"));
    }

    // ── R5: rename / delete (the R3-2 deferred slice) ───────────────────

    async fn seed_two_profiles(state: &AppState, tmp: &tempfile::TempDir) {
        let path = tmp.path().join("providers.toml");
        let mut store = state.provider_store.lock().await;
        *store = ProviderConfigStore::load_or_default_at(&path);
        store.insert_model_profile("default").unwrap();
        store.insert_model_profile("work").unwrap();
        store.insert_model_profile("zzz").unwrap();
        // Active = "work" (non-default) so the fallback and pointer-follow
        // paths are both observable against the same fixture.
        store.set_active_profile_key("work");
        store.save().unwrap();
    }

    #[tokio::test]
    async fn rename_moves_the_entry_and_follows_the_active_pointer() {
        use crate::commands::AppState;

        let state = AppState::new();
        let tmp = tempfile::TempDir::new().unwrap();
        seed_two_profiles(&state, &tmp).await;

        {
            let mut store = state.provider_store.lock().await;
            // The exact mutator `rename_provider_profile` drives, driven the
            // same way (rename of the ACTIVE profile).
            store.rename_model_profile("work", "client-a").unwrap();
            store.save().unwrap();
        }

        let config = {
            let store = state.provider_store.lock().await;
            store.config().clone()
        };
        assert!(!config.profiles.contains_key("work"), "old key gone");
        let renamed = config.profiles.get("client-a").expect("new key present");
        assert_eq!(renamed.name, "client-a", "profile's own name rewritten");
        assert_eq!(
            config.active_profile_key(),
            "client-a",
            "rename follows the active pointer — no silent profile switch"
        );
        let rows = summarize_profiles(&config);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["default", "client-a", "zzz"]);

        // The rename survives a reload.
        let reloaded = ProviderConfigStore::load_or_default_at(&tmp.path().join("providers.toml"));
        assert_eq!(reloaded.config().active_profile_key(), "client-a");
    }

    #[tokio::test]
    async fn rename_errors_map_to_the_engine_kinds() {
        use crate::commands::AppState;

        let state = AppState::new();
        let tmp = tempfile::TempDir::new().unwrap();
        seed_two_profiles(&state, &tmp).await;

        let mut store = state.provider_store.lock().await;
        // Duplicate target → AlreadyExists (the command rewrites this one
        // into "A profile named 'zzz' already exists").
        let dup = store.rename_model_profile("default", "zzz").unwrap_err();
        assert_eq!(dup.kind(), std::io::ErrorKind::AlreadyExists);
        // Unknown source → NotFound (surfaced verbatim by the command).
        let missing = store.rename_model_profile("ghost", "x").unwrap_err();
        assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
        // Neither failed call may leave a partial mutation behind.
        assert!(store.config().profiles.contains_key("default"));
        assert!(store.config().profiles.contains_key("zzz"));
        assert!(!store.config().profiles.contains_key("x"));
    }

    #[tokio::test]
    async fn delete_active_profile_falls_back_to_default_and_reports_it() {
        use crate::commands::AppState;

        let state = AppState::new();
        let tmp = tempfile::TempDir::new().unwrap();
        seed_two_profiles(&state, &tmp).await;

        // force = true is what the command passes (the UI dialog consented).
        let became_active = {
            let mut store = state.provider_store.lock().await;
            let fallback = store.remove_model_profile("work", true).unwrap();
            store.save().unwrap();
            fallback
        };
        assert_eq!(
            became_active.as_deref(),
            Some("default"),
            "engine fallback prefers 'default' when it survives"
        );

        let config = {
            let store = state.provider_store.lock().await;
            store.config().clone()
        };
        assert!(!config.profiles.contains_key("work"));
        assert_eq!(config.active_profile_key(), "default");
        let rows = summarize_profiles(&config);
        assert!(rows.iter().find(|r| r.name == "default").unwrap().active);
        // The outcome wire type would carry exactly this fallback so the UI
        // can say which profile took over.
        assert_eq!(
            DeleteProfileOutcome {
                profiles: rows,
                became_active: became_active.clone(),
            }
            .became_active
            .as_deref(),
            Some("default")
        );

        // The pointer move survives a reload.
        let reloaded = ProviderConfigStore::load_or_default_at(&tmp.path().join("providers.toml"));
        assert_eq!(reloaded.config().active_profile_key(), "default");
    }

    #[tokio::test]
    async fn delete_non_active_profile_reports_none_and_keeps_the_pointer() {
        use crate::commands::AppState;

        let state = AppState::new();
        let tmp = tempfile::TempDir::new().unwrap();
        seed_two_profiles(&state, &tmp).await;

        let mut store = state.provider_store.lock().await;
        let fallback = store.remove_model_profile("zzz", false).unwrap();
        assert_eq!(fallback, None, "inactive delete moves nothing");
        assert_eq!(store.config().active_profile_key(), "work");

        // The two engine refusals the UI must surface:
        // 1. deleting the ACTIVE profile without force is refused;
        let refused = store.remove_model_profile("work", false).unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::InvalidData);
        // 2. deleting the LAST remaining profile is refused.
        store.remove_model_profile("default", true).unwrap();
        let last = store.remove_model_profile("work", true).unwrap_err();
        assert_eq!(last.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            store.config().profiles.contains_key("work"),
            "refusal leaves the profile in place"
        );
    }
}
