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
//! Scope (approved slice): list (name, provider count, active marker),
//! switch (the UI confirms when the target profile is empty), create.
//! NO rename/delete UI this batch — the store already has
//! `rename_model_profile` / `remove_model_profile` for the follow-up.
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
async fn with_model_profile_store<R, F>(
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
}
