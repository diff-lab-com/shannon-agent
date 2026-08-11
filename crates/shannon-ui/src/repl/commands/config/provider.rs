//! `/provider` command handlers — split from `config.rs` (ADR-0008 P2-8).
//!
//! `/provider` lists providers (with key/connected status) and switches the
//! active one; `/provider health` live-probes every allowed provider. The
//! cross-group helpers (`apply_model_selection`, `connected_provider_slugs`,
//! `connect_status`, `parse_provider_name`) and the `HEALTH_PROBE_TIMEOUT`
//! constant live in the parent [`super`] module.

use super::{
    HEALTH_PROBE_TIMEOUT, apply_model_selection, connect_status, connected_provider_slugs,
    parse_provider_name,
};
use crate::repl::Repl;
use crate::{Result, widgets::ChatRole};
use rust_i18n::t;
use shannon_core::model_registry;

pub(crate) fn handle_provider(repl: &mut Repl, args: &str) -> Result<()> {
    if args.trim() == "health" {
        return handle_provider_health(repl);
    }
    if args.is_empty() {
        // List all providers with key status (honours SHANNON_*_PROVIDERS filter)
        let providers = model_registry::available_providers();
        // Same connection set the /connect dashboard and the welcome card use,
        // so the three views agree on which providers are "connected"
        // (ADR-0008 P1-2 / Decision 3).
        let connected = connected_provider_slugs();
        let mut lines = vec![t!("commands.provider.available").to_string()];
        for p in &providers {
            let slug = shannon_core::provider_resolver::llm_provider_id(p);
            let has_key = !p.resolve_api_key_from_env().is_empty();
            // Unified vocabulary (was the divergent "key OK" / "no key" /
            // "no auth" trio). A provider with a stored key but no persisted
            // profile now reads "key stored" here too, matching /connect.
            let status = connect_status(p.requires_auth(), connected.contains(&slug), has_key);
            let current = if repl.state.selected_provider.as_ref() == Some(p) {
                " *"
            } else {
                ""
            };
            lines.push(format!("  {p} — {status}{current}"));
        }
        lines.push(String::new());
        lines.push(t!("commands.provider.legend").to_string());
        repl.chat.add_message(ChatRole::System, lines.join("\n"));
    } else {
        // Switch to specified provider.
        let provider = parse_provider_name(args.trim())?;
        let models = model_registry::merged_models_for_provider(provider.clone());
        let default_model = models.first().map(|m| m.id.to_string());

        // Single switch path. `default_model = None` means "this provider has
        // no built-in catalog (Ollama, OpenRouter, Bedrock, Custom, …)" — the
        // current model id is kept and the user picks via
        // `/model <provider>/<model-id>`.
        apply_model_selection(repl, provider.clone(), default_model.clone(), None, false)?;

        match default_model {
            Some(m) => {
                repl.chat.add_message(
                    ChatRole::System,
                    t!(
                        "commands.provider.switched",
                        provider = &provider.to_string(),
                        model = &m
                    )
                    .to_string(),
                );
            }
            None => {
                repl.chat.add_message(
                    ChatRole::System,
                    t!(
                        "commands.provider.switched_no_catalog",
                        provider = &provider.to_string()
                    )
                    .to_string(),
                );
            }
        }
    }
    Ok(())
}

/// `/provider health` — live-probe every allowed provider and inventory
/// their credential status (ADR-0005 Phase 6 + task 6).
///
/// `engine.probe_all_health()` runs the per-provider endpoint probe
/// concurrently (5s per-provider timeout) and returns a snapshot. The
/// active provider is reported first; if it is unreachable, the command
/// prints a list of reachable candidates as a switch hint — **without**
/// switching automatically (Shannon ships no model router, spec §11).
///
/// Probe is fail-soft: a transport error reports "unreachable" but never
/// crashes the REPL. Bespoke-API providers (Gemini / Bedrock / Azure /
/// Replicate) are skipped because they have no shared list-models endpoint.
fn handle_provider_health(repl: &mut Repl) -> Result<()> {
    use shannon_core::credential_manager::read_credential_value_default;
    use shannon_core::provider_resolver::llm_provider_id;
    use shannon_core::{ProviderHealth, ProviderHealthStatus};

    let connected = connected_provider_slugs();
    let providers = model_registry::available_providers();
    let active = repl.state.selected_provider.clone();
    let active_model = repl.state.model.clone().unwrap_or_else(|| "—".to_string());

    let mut lines = vec!["Provider health:".to_string(), String::new()];

    // 1. Concurrently live-probe every allowed provider (5s each, joined).
    //    Engines run inside catch_unwind so a panic in one provider's probe
    //    can never crash the REPL.
    let probes: Vec<ProviderHealth> = match repl.query_engine.as_ref() {
        Some(engine) => match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            repl.runtime
                .block_on(engine.probe_all_health(HEALTH_PROBE_TIMEOUT))
        })) {
            Ok(probes) => probes,
            // A panic in one provider's probe must not crash the REPL; log it so
            // the (now-missing) health data is diagnosable (ADR-0008 P2-6).
            Err(_) => {
                tracing::error!("probe_all_health panicked (recovered; health data unavailable)");
                Vec::new()
            }
        },
        None => Vec::new(),
    };

    // 2. Active provider verdict first. If absent (no engine or no selection),
    //    skip; the inventory table below still lists everything.
    let active_probe = active
        .as_ref()
        .and_then(|p| probes.iter().find(|h| &h.provider == p));
    if let (Some(provider), Some(probe)) = (active.as_ref(), active_probe) {
        let verdict = match probe.status {
            ProviderHealthStatus::Reachable => format!(
                "● reachable — provider: {provider}, model: {active_model} ({}ms)",
                probe.latency_ms.unwrap_or(0)
            ),
            ProviderHealthStatus::AuthFailed => {
                format!("○ auth rejected — provider: {provider} (key not accepted)")
            }
            ProviderHealthStatus::Unreachable => {
                format!("○ unreachable — provider: {provider}")
            }
            ProviderHealthStatus::NotConfigured => {
                format!("○ not configured — provider: {provider} (no key resolvable)")
            }
        };
        lines.push(verdict);
        lines.push(String::new());
    } else if let (Some(provider), None) = (active.as_ref(), repl.query_engine.as_ref()) {
        lines.push(format!(
            "● {provider} active — query engine not initialized; skipping live probe."
        ));
        lines.push(String::new());
    } else if active.is_none() {
        lines.push("No active provider selected. Use /provider <name> to choose one.".to_string());
        lines.push(String::new());
    }

    // 3. Full per-provider health table — sorted active-first then by name.
    lines.push("All providers (live):".to_string());
    let mut ordered: Vec<&ProviderHealth> = probes.iter().collect();
    ordered.sort_by_key(|h| {
        let is_active = active.as_ref() == Some(&h.provider);
        (!is_active, format!("{:?}", h.provider))
    });
    for h in &ordered {
        let mark = match h.status {
            ProviderHealthStatus::Reachable => "●",
            ProviderHealthStatus::AuthFailed => "○",
            ProviderHealthStatus::Unreachable => "○",
            ProviderHealthStatus::NotConfigured => "·",
        };
        let latency = h
            .latency_ms
            .map(|ms| format!(" ({ms}ms)"))
            .unwrap_or_default();
        let detail = match h.status {
            ProviderHealthStatus::Reachable => "reachable".to_string(),
            ProviderHealthStatus::AuthFailed => "auth rejected".to_string(),
            ProviderHealthStatus::Unreachable => "unreachable".to_string(),
            ProviderHealthStatus::NotConfigured => "not configured".to_string(),
        };
        let active_marker = if active.as_ref() == Some(&h.provider) {
            " *"
        } else {
            ""
        };
        lines.push(format!(
            "  {mark} {provider}{active_marker} — {detail}{latency}",
            provider = h.provider
        ));
    }

    // 4. Switch hint: when the active provider is down, list reachable
    //    candidates the user can switch to. **Manual only** — Shannon has no
    //    model router (spec §11). Pick up to 3 alphabetically.
    if let Some(active_provider) = active.as_ref() {
        let active_status = probes
            .iter()
            .find(|h| &h.provider == active_provider)
            .map(|h| h.status);
        if matches!(
            active_status,
            Some(ProviderHealthStatus::Unreachable | ProviderHealthStatus::AuthFailed)
        ) {
            let candidates: Vec<&ProviderHealth> = probes
                .iter()
                .filter(|h| {
                    h.status == ProviderHealthStatus::Reachable
                        && active.as_ref() != Some(&h.provider)
                })
                .take(3)
                .collect();
            if !candidates.is_empty() {
                let names: Vec<String> =
                    candidates.iter().map(|h| h.provider.to_string()).collect();
                lines.push(String::new());
                lines.push(format!(
                    "Hint: active provider is down. Candidates reachable now: {}. Switch with /provider <name>.",
                    names.join(", ")
                ));
            }
        }
    }

    // 5. Configured-but-unprobed inventory (keeps the credential view from
    //    before task 6 — useful when many providers are NotConfigured).
    lines.push(String::new());
    lines.push("Configured providers:".to_string());
    for p in &providers {
        let slug = llm_provider_id(p);
        let has_key = read_credential_value_default(&slug).is_some();
        let status = connect_status(p.requires_auth(), connected.contains(&slug), has_key);
        let current = if repl.state.selected_provider.as_ref() == Some(p) {
            " *"
        } else {
            ""
        };
        lines.push(format!("  {p}{current} — {status}"));
    }
    lines.push(String::new());
    lines.push(
        "Probes run concurrently (5s each). Switch with /provider <name> or /connect.".to_string(),
    );
    repl.chat.add_message(ChatRole::System, lines.join("\n"));
    Ok(())
}
