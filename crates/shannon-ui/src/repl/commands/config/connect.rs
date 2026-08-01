//! `/connect` + `/disconnect` command handlers — split from `config.rs`
//! (ADR-0008 P2-8).
//!
//! `apply_connect` is a thin orchestrator over seven steps (ADR-0008 P3-4):
//! `store_credential` → `persist_profile` → `apply_model_selection` (shared,
//! parent) → `validate_credential` → `reload_credential` (inline) →
//! `spawn_refresh` → `open_picker`. The cross-group helpers
//! (`apply_model_selection`, `connect_status`, `connected_provider_slugs`,
//! `parse_provider_name`, `sync_active_to_chat`) and the
//! `CONNECT_REFRESH_TIMEOUT` constant live in the parent [`super`] module.

use super::{
    CONNECT_REFRESH_TIMEOUT, apply_model_selection, connect_status, connected_provider_slugs,
    parse_provider_name, sync_active_to_chat,
};
use crate::repl::Repl;
use crate::{Result, widgets::ChatRole};
use rust_i18n::t;
use shannon_core::model_registry;
use shannon_engine::api::LlmProvider;
use std::ops::ControlFlow;

/// Parsed `/connect` arguments (ADR-0005 Phase 4). Pure — no side effects, so
/// it is unit-testable without a `Repl`.
pub(crate) struct ConnectArgs<'a> {
    /// First token (provider name or alias), e.g. `anthropic` / `glm`.
    pub provider_arg: &'a str,
    /// Remainder after the provider token, trimmed — the API key if given.
    pub key_arg: Option<&'a str>,
}

/// Split `/connect` args into `(provider, optional key)`. Returns `None` for
/// empty/whitespace-only input so the caller can show help. The key is the
/// full remainder after the first whitespace run (API keys contain no
/// spaces), with surrounding whitespace trimmed.
pub(crate) fn parse_connect_args(args: &str) -> Option<ConnectArgs<'_>> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let provider_arg = parts.next().unwrap_or("");
    let key_arg = parts.next().map(str::trim).filter(|s| !s.is_empty());
    Some(ConnectArgs {
        provider_arg,
        key_arg,
    })
}
/// No-arg `/connect` dashboard: list every provider with its connection status
/// plus a one-line syntax hint. Replaces the old wall-of-text help — detailed
/// docs live at `/help connect`. Short lines so the chat panel never wraps it
/// into the jagged layout the long help string produced.
fn show_connect_dashboard(repl: &mut Repl) {
    use shannon_core::credential_manager::read_credential_value_default;
    use shannon_core::provider_resolver::llm_provider_id;

    let connected = connected_provider_slugs();
    let mut lines = vec![
        t!("commands.connect.dashboard_header").to_string(),
        String::new(),
        t!("commands.connect.dashboard_providers").to_string(),
    ];
    for p in model_registry::available_providers() {
        let slug = llm_provider_id(&p);
        let has_key = read_credential_value_default(&slug).is_some();
        let status = connect_status(p.requires_auth(), connected.contains(&slug), has_key);
        let current = if repl.state.selected_provider.as_ref() == Some(&p) {
            " *"
        } else {
            ""
        };
        lines.push(format!("  {p}{current} — {status}"));
    }
    lines.push(String::new());
    lines.push(t!("commands.connect.dashboard_usage").to_string());
    lines.push(t!("commands.connect.dashboard_help").to_string());
    repl.chat.add_message(ChatRole::System, lines.join("\n"));
}

/// `/connect <provider> [api-key]` — connect a provider with no environment
/// variable (ADR-0005 Phase 4).
///
/// - `/connect` (no args)        → dashboard listing every provider + status.
/// - `/connect <provider>`       → if the provider needs a key and none is
///   stored, prints a guidance message pointing the user to the inline form
///   `/connect <provider> <your-api-key>`. If a key is already stored (or the
///   provider needs no auth), connects immediately.
/// - `/connect <provider> <key>` → the key is taken from the argument and the
///   connection is applied directly. This is the recommended path.
///
/// Security: when a key is passed inline, `redact_secret_command` (see
/// `commands/mod.rs`) replaces it with `***` before the user's message is added
/// to the chat widget or command history, so the plaintext key never lands in
/// the session JSON. The real key still reaches `apply_connect` for execution.
///
/// The stored key lives only in `~/.shannon/credentials/<service>.json` (0600)
/// — never in a config file (decision A1). The persisted v2 provider config
/// (`CredentialRef::Store`) activates on the next launch via the `ConfigBuilder`
/// connected layer (which wins over ambient `SHANNON_*` env vars), so the
/// connection is durable with zero env vars.
pub(crate) fn handle_connect(repl: &mut Repl, args: &str) -> Result<()> {
    let parsed = match parse_connect_args(args) {
        Some(p) => p,
        None => {
            show_connect_dashboard(repl);
            return Ok(());
        }
    };
    let ConnectArgs {
        provider_arg,
        key_arg,
    } = parsed;

    let provider = match parse_provider_name(provider_arg) {
        Ok(p) => p,
        Err(e) => {
            super::super::set_error(repl, &e.to_string());
            return Ok(());
        }
    };

    // Auth required, no inline key, nothing stored → guide the user to the
    // inline form instead of opening a dialog. The inline path is the
    // recommended one because `redact_secret_command` keeps the typed key out
    // of the conversation/session JSON.
    let has_stored_key = has_connect_key(&provider);
    if should_prompt_for_key(provider.requires_auth(), key_arg, has_stored_key) {
        guide_to_inline_connect(repl, provider_arg);
        return Ok(());
    }

    apply_connect(repl, provider, key_arg)
}

/// Whether `/connect` should show the no-key guidance instead of connecting
/// immediately. Pure — unit-tested below.
///
/// True only when the provider needs a key, none was passed inline, and none is
/// already stored. In every other case (no-auth provider, inline key, or an
/// existing stored key) we connect right away.
pub(super) fn should_prompt_for_key(
    requires_auth: bool,
    key_arg: Option<&str>,
    has_stored_key: bool,
) -> bool {
    requires_auth && key_arg.is_none() && !has_stored_key
}

/// Whether a key is already stored for `provider`'s credential service.
fn has_connect_key(provider: &LlmProvider) -> bool {
    let service = shannon_core::provider_resolver::llm_provider_id(provider);
    shannon_core::credential_manager::read_credential_value_default(&service).is_some()
}

/// Persist the key (if any) + v2 profile and switch the running engine + REPL
/// state to the provider's default model. Shared by the inline-key path and the
/// wizard's submit handler.
///
/// Thin orchestrator over seven steps (ADR-0008 P3-4 split): `store_credential`
/// → `persist_profile` → [`apply_model_selection`] → `validate_credential` →
/// `reload_credential` (inline) → `spawn_refresh` → `open_picker`. Each step
/// that can fail returns a [`std::ops::ControlFlow`]; `Break` means the error
/// was already surfaced via `set_error` and the flow stops (returns `Ok(())`).
///
/// `key == None` means "no new key supplied" — the caller has already verified
/// a key is stored (or the provider needs no auth), so we just reuse what's on
/// disk. Decision A1: a plaintext key is written only to
/// `~/.shannon/credentials/<service>.json` (0600), never to a config file.
pub(crate) fn apply_connect(
    repl: &mut Repl,
    provider: LlmProvider,
    key: Option<&str>,
) -> Result<()> {
    use shannon_core::provider_resolver::llm_provider_id;

    let display = format!("{provider}");
    let service = llm_provider_id(&provider);
    let mut lines: Vec<String> = Vec::new();

    // 1. API key → credential store (idempotent; plaintext lands only on disk
    //    at ~/.shannon/credentials/<service>.json, 0600 — never in a config).
    if store_credential(repl, &service, &display, key, &mut lines).is_break() {
        return Ok(());
    }

    // 2. Persist the v2 provider config via the single write path — an additive
    //    upsert, so connecting a second provider no longer drops the first
    //    (ADR-0008 P2-5 step 4 / Decision 3).
    let model_id = match persist_profile(repl, &provider, &mut lines) {
        ControlFlow::Continue(id) => id,
        ControlFlow::Break(()) => return Ok(()),
    };

    // 3. Switch the running engine + REPL state to the provider's default model
    //    via the single switch path (mirrors /provider). Updates the client's
    //    provider/model/base_url; the key is swapped in step 5 below via
    //    reload_credential (ADR-0008 Decision 4).
    let _ = apply_model_selection(repl, provider.clone(), Some(model_id.clone()), None, false)?;

    // 4. Validate the credential with a 1-token probe so a bad key/region/model
    //    fails at connect time. Hard auth failure aborts (chat already flushed);
    //    other errors warn but keep the connection.
    let probe_key = match validate_credential(
        repl, &provider, &service, &display, &model_id, key, &mut lines,
    ) {
        ControlFlow::Continue(k) => k,
        ControlFlow::Break(()) => return Ok(()),
    };

    // 5. Hot-reload the running client with the resolved key so the next query
    //    uses it immediately — no restart (ADR-0008 Decision 4 / P1-1). The
    //    client config already carries the switched provider/base_url/model from
    //    step 3; reload_credential only swaps the key. Skipped when there is no
    //    key to load (no-auth providers, or a connect that reused nothing).
    if let Some(api_key) = probe_key.as_deref() {
        if let Some(engine) = repl.query_engine.as_mut() {
            engine.reload_credential(api_key);
        }
    }

    lines.push(
        t!(
            "commands.connect.switched",
            provider = &provider.to_string(),
            model = &model_id
        )
        .to_string(),
    );
    repl.chat.add_message(ChatRole::System, lines.join("\n"));

    // 6. Spawn a non-blocking models.dev refresh so the picker (next step) can
    //    show freshly discovered models (ADR-0008 P3-4 — extracted step fn).
    spawn_refresh(repl);

    // 7. Open the model picker on the freshly connected provider so the user
    //    can confirm or change the default model (ADR-0008 P3-4).
    open_picker(repl, &model_id);

    Ok(())
}

/// Step 1 of [`apply_connect`]: write the new API key (if any) to the credential
/// store, or note that we're reusing the stored one (ADR-0008 P3-4).
///
/// Returns `Break` if the store failed (error already shown via `set_error`);
/// `Continue` otherwise. Appends a user-facing line on success. Decision A1: a
/// plaintext key is written only to `~/.shannon/credentials/<service>.json`
/// (0600), never to a config file. The no-key case is intercepted upstream
/// (`guide_to_inline_connect`); no-auth providers intentionally print nothing.
fn store_credential(
    repl: &mut Repl,
    service: &str,
    display: &str,
    key: Option<&str>,
    lines: &mut Vec<String>,
) -> ControlFlow<()> {
    use shannon_core::credential_manager::{
        Credential, CredentialManager, read_credential_value_default,
    };

    if let Some(new_key) = key.filter(|k| !k.is_empty()) {
        match CredentialManager::new()
            .and_then(|mut m| m.store_or_update(Credential::new(service, service, new_key)))
        {
            Ok(_) => lines.push(
                t!(
                    "commands.connect.key_stored",
                    provider = display,
                    service = service
                )
                .to_string(),
            ),
            Err(e) => {
                super::super::set_error(
                    repl,
                    &t!("commands.connect.storing_error", error = &e.to_string()),
                );
                return ControlFlow::Break(());
            }
        }
    } else if read_credential_value_default(service).is_some() {
        lines.push(
            t!(
                "commands.connect.reusing_key",
                provider = display,
                service = service
            )
            .to_string(),
        );
    }
    ControlFlow::Continue(())
}

/// Step 2 of [`apply_connect`]: upsert the provider config via the single write
/// path (`ProviderConfigService::connect`) — an additive merge, so a second
/// `/connect` no longer drops the first (ADR-0008 P2-5 step 4 / Decision 3).
///
/// Returns `Continue(model_id)` (catalog default; the engine loads the config on
/// next launch, no env var required) or `Break` if persistence failed (error
/// already shown). Appends a "config saved" line on success.
fn persist_profile(
    repl: &mut Repl,
    provider: &LlmProvider,
    lines: &mut Vec<String>,
) -> ControlFlow<(), String> {
    use shannon_core::provider_config_service::ProviderConfigService;

    let mut svc = ProviderConfigService::load();
    let connected = match svc.connect(provider.clone(), None, None, true) {
        Ok(c) => c,
        Err(e) => {
            super::super::set_error(
                repl,
                &t!("commands.connect.saving_error", error = &e.to_string()),
            );
            return ControlFlow::Break(());
        }
    };
    let model_id = connected.model_id;
    lines.push(
        t!(
            "commands.connect.config_saved",
            path = &connected.saved_path.display().to_string()
        )
        .to_string(),
    );
    ControlFlow::Continue(model_id)
}

/// Step 4 of [`apply_connect`]: probe the credential with a 1-token request so a
/// bad key/region/model fails at connect time, not mid-query (ADR-0008 P3-4).
///
/// Fail-soft: a non-auth error warns but keeps the connection (it may be
/// transient). A hard auth failure flushes the accumulated `lines` to chat, sets
/// the error, and signals `Break` so the caller stops without printing a
/// misleading "✓ Switched". Returns `Continue(probe_key)` — the resolved key
/// (new or reused), which step 5 feeds into `engine.reload_credential`.
fn validate_credential(
    repl: &mut Repl,
    provider: &LlmProvider,
    service: &str,
    display: &str,
    model_id: &str,
    key: Option<&str>,
    lines: &mut Vec<String>,
) -> ControlFlow<(), Option<String>> {
    use shannon_core::credential_manager::read_credential_value_default;

    let probe_key = key
        .filter(|k| !k.is_empty())
        .map(str::to_string)
        .or_else(|| read_credential_value_default(service));
    if provider.requires_auth() {
        if let (Some(api_key), Some(engine)) = (probe_key.as_deref(), repl.query_engine.as_ref()) {
            let probed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                repl.runtime.block_on(engine.validate_credential(api_key))
            }));
            match probed {
                Ok(Ok(())) => {
                    lines.push(t!("commands.connect.cred_verified", model = model_id).to_string())
                }
                Ok(Err(shannon_engine::api::ApiError::AuthenticationFailed)) => {
                    lines.push(t!("commands.connect.auth_failed", provider = display).to_string());
                    repl.chat.add_message(ChatRole::System, lines.join("\n"));
                    super::super::set_error(
                        repl,
                        &t!("commands.connect.auth_failed", provider = display),
                    );
                    return ControlFlow::Break(());
                }
                Ok(Err(e)) => lines.push(
                    t!(
                        "commands.connect.verify_warning",
                        provider = display,
                        error = &e.to_string()
                    )
                    .to_string(),
                ),
                Err(_) => {
                    lines.push(t!("commands.connect.probe_failed", provider = display).to_string())
                }
            }
        }
    }
    ControlFlow::Continue(probe_key)
}

/// Step 6 of [`apply_connect`]: spawn a non-blocking models.dev refresh so the
/// picker (next step) can show freshly discovered models. `CONNECT_REFRESH_TIMEOUT`
/// — if the user is offline the static catalog remains authoritative and the
/// picker falls back to it transparently. Errors are swallowed by design
/// (ADR-0008 P3-4). Explicit `drop` to satisfy `clippy::let_underscore_future`:
/// the `JoinHandle` is intentionally discarded (we don't await or report it).
fn spawn_refresh(repl: &mut Repl) {
    std::mem::drop(repl.runtime.spawn(async move {
        let _ =
            shannon_core::model_registry::dynamic::refresh_overlay_async(CONNECT_REFRESH_TIMEOUT)
                .await;
    }));
}

/// Step 7 of [`apply_connect`]: open the model picker on the freshly connected
/// provider so the user can confirm or change the default model. Enter commits
/// the selection (overwriting `model_id`); Esc keeps `model_id` (already applied
/// at step 3). Both paths are non-breaking (ADR-0008 P3-4).
fn open_picker(repl: &mut Repl, model_id: &str) {
    let picker = crate::widgets::select::ModelPickerWidget::new(Some(model_id));
    repl.state.model_picker = Some(picker);
}

/// `/disconnect <provider>` — remove a provider's persisted provider config
/// (and thus its "connected" state) so it no longer appears as connected in
/// the welcome card or `/connect` dashboard (ADR-0008 P1-4).
///
/// Removes the provider slot from the `"default"` model profile (clearing
/// `active_target` if it pointed there) and persists via the single write
/// path — [`ProviderConfigService::disconnect`] (ADR-0008 P2-5 step 3). When
/// the disconnected provider was the active selection, the REPL switches to
/// the next still-connected provider the service names (deterministic: first
/// remaining), or falls back to the unconfigured state if none remain. The
/// on-disk credential is intentionally kept so a subsequent `/connect` is a
/// one-step re-connect.
pub(crate) fn handle_disconnect(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::provider_config_service::ProviderConfigService;
    use shannon_core::provider_resolver::llm_provider_from_slug;

    let name = args.trim();
    if name.is_empty() {
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.disconnect.usage").to_string(),
        );
        return Ok(());
    }

    let provider = parse_provider_name(name)?;
    let display = format!("{provider}");

    // Remove the slot through the single write path. The service clears
    // `active_target` when it pointed at the removed slot, so the persisted
    // config never references a missing provider. Idempotent —
    // `was_connected` reports whether there was a slot to remove.
    let mut svc = ProviderConfigService::load();
    let outcome = match svc.disconnect(&provider) {
        Ok(o) => o,
        Err(e) => {
            super::super::set_error(
                repl,
                &t!("commands.connect.saving_error", error = &e.to_string()),
            );
            return Ok(());
        }
    };

    if !outcome.was_connected {
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.disconnect.not_connected", provider = &display).to_string(),
        );
        return Ok(());
    }

    // If we just disconnected the active selection, switch to the next
    // still-connected provider; otherwise just refresh the card. The service
    // picks the first remaining slug (deterministic); we resolve its default
    // model through the single switch path so state, engine, and card stay in
    // sync. (Session concern — the candidate comes from the service, the
    // engine switch stays in the REPL.)
    let was_active = repl.state.selected_provider.as_ref() == Some(&provider);
    let mut lines = vec![t!("commands.disconnect.done", provider = &display).to_string()];
    if was_active {
        match outcome
            .next_active
            .as_deref()
            .and_then(llm_provider_from_slug)
        {
            Some(p) => {
                let default_model = model_registry::merged_models_for_provider(p.clone())
                    .first()
                    .map(|m| m.id.to_string());
                let _ = apply_model_selection(repl, p.clone(), default_model.clone(), None, false)?;
                lines.push(
                    t!(
                        "commands.disconnect.switched",
                        provider = &p.to_string(),
                        model = default_model.as_deref().unwrap_or("—")
                    )
                    .to_string(),
                );
            }
            None => {
                repl.state.selected_provider = None;
                sync_active_to_chat(repl);
                lines.push(t!("commands.disconnect.none_remain").to_string());
            }
        }
    } else {
        sync_active_to_chat(repl);
    }
    repl.chat.add_message(ChatRole::System, lines.join("\n"));
    Ok(())
}

/// Print a guidance message pointing the user to the inline connect form.
///
/// We deliberately do NOT open an API-key input dialog: the inline form
/// `/connect <provider> <your-api-key>` is the recommended path, and
/// `redact_secret_command` (in `commands/mod.rs`) ensures the typed key is
/// never persisted into the conversation or command history. `provider_arg` is
/// the user's own input so the example matches what they typed.
fn guide_to_inline_connect(repl: &mut Repl, provider_arg: &str) {
    repl.chat.add_message(
        ChatRole::System,
        format!(
            "This provider needs an API key. Connect it with:\n\n    /connect {provider_arg} <your-api-key>\n\nThe key is stored on disk (0600) and is never recorded in the conversation."
        ),
    );
}
