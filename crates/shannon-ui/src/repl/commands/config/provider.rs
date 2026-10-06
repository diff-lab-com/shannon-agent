//! `/provider` command handlers — split from `config.rs` (ADR-0008 P2-8).
//!
//! `/provider` lists providers (with key/connected status) and switches the
//! active one; `/provider health` live-probes every allowed provider. The
//! switch is tightened per ruling ⑤ (review P-N14 / 09-30 roadmap B9):
//! switching to a provider with no catalog entries no longer silently keeps
//! the previous model — it forces the model picker, or the user gives an
//! explicit `/provider <name> <model-id>`. The cross-group helpers
//! (`apply_model_selection`, `connected_provider_slugs`, `connect_status`,
//! `parse_provider_name`) and the `HEALTH_PROBE_TIMEOUT` constant live in
//! the parent [`super`] module.

use super::{
    HEALTH_PROBE_TIMEOUT, apply_model_selection, connect_status, connected_provider_slugs,
    parse_provider_name,
};
use crate::repl::Repl;
use crate::{Result, widgets::ChatRole};
use rust_i18n::t;
use shannon_core::model_registry;
use shannon_engine::api::LlmProvider;

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
        // Switch to the specified provider. Grammar (ruling ⑤, review
        // P-N14 / 09-30 roadmap B9): `/provider <name>` or, when the target
        // needs an explicit model, `/provider <name> <model-id>`.
        let (provider_token, explicit_model) = parse_provider_args(args);
        let provider = parse_provider_name(provider_token)?;
        let models = model_registry::merged_models_for_provider(provider.clone());

        match plan_switch(explicit_model.as_deref(), models.first().map(|m| m.id)) {
            ProviderSwitchDecision::UseModel(model_id) => {
                // Single switch path (ADR-0008 Decision 2). An explicit id is
                // always honored — the user named the exact target (ruling ⑤:
                // "explicit full id passes"); a bare switch lands on the
                // provider's first catalog entry as before.
                apply_explicit_provider_model(repl, provider, &model_id)?;
            }
            ProviderSwitchDecision::ForcePicker => {
                // Ruling ⑤: a provider with no catalog entry must not
                // silently keep the previous provider's model — that ships a
                // misconfiguration ("带着错误配置走"). The switch is refused
                // here: nothing changes until the user picks a model, and a
                // picker Enter completes provider+model together through the
                // single switch path. Esc leaves the current setup intact.
                let mut picker =
                    crate::widgets::select::ModelPickerWidget::new(repl.state.model.as_deref());
                picker.focus_provider(&provider);
                repl.state.model_picker = Some(picker);
                repl.chat.add_message(
                    ChatRole::System,
                    t!(
                        "commands.provider.no_catalog_pick_required",
                        provider = &provider.to_string()
                    )
                    .to_string(),
                );
            }
        }
    }
    Ok(())
}

/// Land an explicit model id on `provider` through the single switch path
/// (ADR-0008 Decision 2), with the "switched" message and the foreign-model
/// warning (review P1-8 copy: an explicit cross-provider id is allowed but
/// must stay visible).
///
/// Shared by the two explicit-id entry points so they cannot drift: the
/// `/provider <name> <model-id>` grammar and the ForcePicker manual-entry
/// confirm (ruling ⑤ follow-up: an id typed into the forced picker is the
/// same explicit id for the same target provider, just entered
/// interactively).
pub(crate) fn apply_explicit_provider_model(
    repl: &mut Repl,
    provider: LlmProvider,
    model_id: &str,
) -> Result<()> {
    apply_model_selection(
        repl,
        provider.clone(),
        Some(model_id.to_string()),
        None,
        false,
    )?;
    repl.chat.add_message(
        ChatRole::System,
        t!(
            "commands.provider.switched",
            provider = &provider.to_string(),
            model = model_id
        )
        .to_string(),
    );
    if let Some(warning) = foreign_model_warning(Some(model_id), None, &provider) {
        repl.chat.add_message(ChatRole::System, warning);
    }
    Ok(())
}

/// Split `/provider` args into `(provider-slug, explicit-model-id)`.
///
/// The model id may itself contain `/` (OpenRouter-style `vendor/model`
/// ids), so only the FIRST whitespace-delimited token is the provider slug
/// and the rest is joined back verbatim. Pure — unit-tested below.
fn parse_provider_args(args: &str) -> (&str, Option<String>) {
    let args = args.trim();
    match args.split_once(char::is_whitespace) {
        Some((slug, rest)) => (slug, Some(rest.trim().to_string())),
        None => (args, None),
    }
}

/// The decision a `/provider` switch resolves to (ruling ⑤, review P-N14 /
/// 09-30 roadmap B9). Pure — unit-tested below.
pub(crate) enum ProviderSwitchDecision {
    /// Switch and land on this model: the user's explicit id, or — for a
    /// bare `/provider <name>` — the provider's first catalog entry.
    UseModel(String),
    /// Bare switch onto a provider with **no catalog entry**. This used to
    /// silently keep the previous provider's model (a misconfiguration
    /// carried forward); it is now refused — the caller forces the model
    /// picker so the user lands an explicit id instead.
    ForcePicker,
}

/// Resolve a `/provider` switch into a [`ProviderSwitchDecision`].
///
/// An explicit (non-empty) model id always wins — the user gave the full
/// target, so the switch proceeds regardless of catalog coverage. Otherwise
/// a catalog default (first merged entry) keeps the pre-tightening
/// first-entry behavior, and a provider with no entries at all forces the
/// picker. Pure — unit-tested below.
fn plan_switch(
    explicit_model: Option<&str>,
    catalog_default: Option<&str>,
) -> ProviderSwitchDecision {
    if let Some(id) = explicit_model.map(str::trim).filter(|s| !s.is_empty()) {
        return ProviderSwitchDecision::UseModel(id.to_string());
    }
    match catalog_default {
        Some(id) => ProviderSwitchDecision::UseModel(id.to_string()),
        None => ProviderSwitchDecision::ForcePicker,
    }
}

/// Compose the foreign-model warning for an explicit `/provider <name>
/// <model-id>` switch (review P1-8 copy, re-wired by S2-6 ruling ⑤).
///
/// The pre-tightening switch kept the current model on a catalog-less
/// provider; ruling ⑤ replaced that with [`plan_switch`]'s force-picker
/// refusal. The warning copy survives for the one path where a model id can
/// still travel across providers: the user's **explicit** id. When the
/// catalog attributes that id to a provider *other than* the target, the
/// warning makes the cross-provider trip visible — the switch itself stays
/// allowed because it was explicit. Ownership comes from the catalog first
/// (`model_info_for_alias`, which also expands aliases); ids unknown to the
/// catalog attribute to nothing and stay unwarned. Pure — unit-tested below.
fn foreign_model_warning(
    current_model: Option<&str>,
    current_provider: Option<&LlmProvider>,
    target: &LlmProvider,
) -> Option<String> {
    let model = current_model?.trim();
    if model.is_empty() {
        return None;
    }
    let owner = model_registry::model_info_for_alias(model)
        .map(|info| &info.provider)
        .or(current_provider)?;
    if owner == target {
        return None;
    }
    Some(
        t!(
            "commands.provider.foreign_model_warning",
            model = model,
            old_provider = shannon_core::provider_resolver::llm_provider_id(owner),
            new_provider = shannon_core::provider_resolver::llm_provider_id(target),
        )
        .to_string(),
    )
}

/// Providers the concurrent probe cannot cover, one formatted line each.
///
/// `probe_all_health` silently drops providers whose list-models API has no
/// shared probeable endpoint per `probe_kind_for_provider` (currently Gemini
/// and Bedrock; everything OpenAI-wire — including Azure and Replicate — is
/// probeable via the openai-compatible `/models` endpoint), so silence in the
/// live table would be indistinguishable from health. This reports each
/// skipped provider explicitly, sorted by name. A provider already present in
/// `probes` (e.g. a keyless Gemini reporting NotConfigured) is not repeated
/// here. Pure — unit-tested below.
fn health_skipped_lines(
    providers: &[LlmProvider],
    probes: &[shannon_core::ProviderHealth],
) -> Vec<String> {
    use shannon_engine::api::probe::probe_kind_for_provider;

    let mut skipped: Vec<&LlmProvider> = providers
        .iter()
        .filter(|p| {
            probe_kind_for_provider(p).is_none() && !probes.iter().any(|h| &h.provider == *p)
        })
        .collect();
    skipped.sort_by_key(|p| p.to_string());
    skipped
        .iter()
        .map(|p| {
            t!(
                "commands.provider.health_skipped_line",
                provider = p.to_string()
            )
            .to_string()
        })
        .collect()
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
/// crashes the REPL. Providers without a probeable list-models endpoint (per
/// `probe_kind_for_provider` — currently Gemini / Bedrock) are skipped; each
/// of them is printed with its own skip line instead of disappearing from
/// the report (R1-5).
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

    // 3b. Skipped providers (R1-5): the probe only covers providers with a
    //     shared list-models endpoint; report each bespoke-API provider so
    //     its absence from the table above is explained, not silent.
    let skipped = health_skipped_lines(&providers, &probes);
    if !skipped.is_empty() {
        lines.push(String::new());
        lines.push(t!("commands.provider.health_skipped_header").to_string());
        lines.extend(skipped);
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── health_skipped_lines (R1-5: /provider health explains its skips) ──

    #[test]
    fn skipped_lines_cover_every_bespoke_api_provider() {
        let providers = vec![
            LlmProvider::Anthropic,
            LlmProvider::Ollama,
            LlmProvider::Gemini,
            LlmProvider::Bedrock,
            // Azure / Replicate share the OpenAI wire format, so the engine's
            // probe covers them via the openai-compatible /models endpoint —
            // they must never be reported as skipped (the pre-R1-5 roadmap
            // text listing them as skipped predates that probe coverage).
            LlmProvider::Azure,
            LlmProvider::Replicate,
        ];
        let lines = health_skipped_lines(&providers, &[]);

        // Exactly the providers `probe_kind_for_provider` cannot cover:
        // Gemini (bespoke Gemini API) and Bedrock (Anthropic-wire signing).
        assert_eq!(lines.len(), 2, "one line per skipped provider: {lines:?}");
        let joined = lines.join("\n");
        for name in ["bedrock", "gemini"] {
            assert!(
                lines
                    .iter()
                    .any(|l| l.contains(name) && l.contains("skipped")),
                "each skipped provider gets its own reason line ({name}): {joined}"
            );
        }
        // Probeable providers are never reported as skipped — including the
        // openai-wire ones.
        for name in ["anthropic", "ollama", "azure", "replicate"] {
            assert!(!joined.contains(name), "{name}: {joined}");
        }
        // Sorted by name for a stable report.
        let names: Vec<String> = lines
            .iter()
            .map(|l| {
                l.split("—")
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .trim_start_matches('·')
                    .trim()
                    .to_string()
            })
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "skipped lines are name-sorted: {names:?}");
    }

    #[test]
    fn skipped_lines_do_not_repeat_probed_providers() {
        use shannon_core::{ProviderHealth, ProviderHealthStatus};

        let providers = vec![LlmProvider::Gemini, LlmProvider::Anthropic];
        // A keyless Gemini shows up in the live table as NotConfigured —
        // it must not be reported as skipped a second time.
        let probes = vec![ProviderHealth {
            provider: LlmProvider::Gemini,
            status: ProviderHealthStatus::NotConfigured,
            latency_ms: None,
        }];
        assert!(health_skipped_lines(&providers, &probes).is_empty());
    }

    // ── foreign_model_warning (review P1-8 copy; since ruling ⑤ wired to
    //    the explicit `/provider <name> <model-id>` path — the only place a
    //    model id can still cross providers) ─────────────────────────────────

    #[test]
    fn warning_fires_for_catalog_model_moving_to_a_different_provider() {
        let out = foreign_model_warning(
            Some("claude-sonnet-4-20250514"),
            Some(&LlmProvider::Anthropic),
            &LlmProvider::Ollama,
        )
        .expect("warning expected for a cross-provider kept model");
        assert!(out.contains("claude-sonnet-4-20250514"), "got {out}");
        assert!(out.contains("anthropic"), "got {out}");
        assert!(out.contains("ollama"), "got {out}");
        // The exact fix command must be pasteable.
        assert!(out.contains("/model ollama/<model-id>"), "got {out}");
    }

    #[test]
    fn warning_fires_for_explicit_foreign_id_without_current_provider() {
        // Ruling ⑤ explicit path: `/provider ollama claude-sonnet-4-20250514`
        // passes current_provider=None (the id is given, not inherited), so
        // ownership must come from the catalog alone.
        let out =
            foreign_model_warning(Some("claude-sonnet-4-20250514"), None, &LlmProvider::Ollama)
                .expect("warning expected: explicit id belongs to another provider");
        assert!(out.contains("anthropic"), "got {out}");
    }

    #[test]
    fn warning_resolves_alias_to_owner_provider() {
        // A bare alias ("sonnet") must attribute to its catalog owner
        // (Anthropic), not stay unattributed.
        let out = foreign_model_warning(Some("sonnet"), None, &LlmProvider::Replicate)
            .expect("warning expected: alias resolves to a foreign owner");
        assert!(out.contains("anthropic"), "got {out}");
        assert!(out.contains("replicate"), "got {out}");
    }

    #[test]
    fn warning_quiet_when_owner_equals_target() {
        // Switching within the same provider cannot misroute the model.
        assert_eq!(
            foreign_model_warning(
                Some("claude-sonnet-4-20250514"),
                Some(&LlmProvider::Anthropic),
                &LlmProvider::Anthropic,
            ),
            None
        );
    }

    #[test]
    fn warning_unknown_model_falls_back_to_selected_provider() {
        // An id the catalog does not know is attributed to the currently
        // selected provider (where it was actually being used).
        let out = foreign_model_warning(
            Some("my-fine-tune"),
            Some(&LlmProvider::OpenAI),
            &LlmProvider::Anthropic,
        )
        .expect("warning expected: selected provider differs from target");
        assert!(out.contains("openai"), "got {out}");
        // With nothing selected there is nothing attributable → no warning.
        assert_eq!(
            foreign_model_warning(Some("llama3"), None, &LlmProvider::Ollama),
            None
        );
    }

    #[test]
    fn warning_none_without_a_current_model() {
        assert_eq!(
            foreign_model_warning(None, None, &LlmProvider::Ollama),
            None
        );
        // A blank/whitespace model id is treated as "no model".
        assert_eq!(
            foreign_model_warning(Some("   "), None, &LlmProvider::Ollama),
            None
        );
    }

    // ── Ruling ⑤ (review P-N14 / 09-30 roadmap B9): a bare switch onto a
    //    provider with no catalog entry must never land as "old model kept".
    //    The decision is pure, so the contract is pinned here. ────────────

    #[test]
    fn explicit_model_always_wins_even_without_catalog() {
        // An explicit id is the user's full target — honored regardless of
        // catalog coverage ("explicit full id passes").
        assert!(matches!(
            plan_switch(Some("my-azure-deployment"), None),
            ProviderSwitchDecision::UseModel(id) if id == "my-azure-deployment"
        ));
        // An explicit id beats the catalog default too.
        assert!(matches!(
            plan_switch(Some("my-tune"), Some("gpt-4o-azure")),
            ProviderSwitchDecision::UseModel(id) if id == "my-tune"
        ));
    }

    #[test]
    fn bare_switch_uses_first_catalog_entry() {
        // With catalog coverage the pre-tightening behavior stands: first
        // merged entry. (Azure itself ships ≥4 entries — pinned by
        // `gap_providers_have_catalog_entries` in shannon-core — so the
        // ForcePicker arm below is for the genuinely catalog-less.)
        assert!(matches!(
            plan_switch(None, Some("gpt-5-azure")),
            ProviderSwitchDecision::UseModel(id) if id == "gpt-5-azure"
        ));
    }

    #[test]
    fn bare_switch_without_catalog_forces_picker_instead_of_silent_keep() {
        assert_eq!(
            std::mem::discriminant(&plan_switch(None, None)),
            std::mem::discriminant(&ProviderSwitchDecision::ForcePicker),
            "no catalog + nothing explicit must refuse the silent keep"
        );
        // A blank explicit id counts as "nothing explicit".
        assert_eq!(
            std::mem::discriminant(&plan_switch(Some("   "), None)),
            std::mem::discriminant(&ProviderSwitchDecision::ForcePicker),
        );
    }

    #[test]
    fn azure_bare_switch_now_lands_on_catalog_default() {
        // S2-6 end-to-end shape: after the catalog batch, `/provider azure`
        // resolves through UseModel — the P-N14 vacuum is closed.
        let models = model_registry::merged_models_for_provider(LlmProvider::Azure);
        let decision = plan_switch(None, models.first().map(|m| m.id));
        assert!(
            matches!(decision, ProviderSwitchDecision::UseModel(_)),
            "azure must have catalog defaults now"
        );
    }

    // ── Explicit-id landing path shared by the `/provider <name> <model-id>`
    //    grammar and the ForcePicker manual-entry confirm (ruling ⑤
    //    follow-up): the helper alone must perform the full switch. ────────

    #[test]
    fn explicit_provider_model_helper_switches_provider_and_model() {
        let mut repl = Repl::new().unwrap();
        repl.state.model = Some("claude-sonnet-4-20250514".to_string());
        repl.state.selected_provider = Some(LlmProvider::Anthropic);

        apply_explicit_provider_model(&mut repl, LlmProvider::Bedrock, "my-tune").unwrap();

        assert_eq!(
            repl.state.selected_provider,
            Some(LlmProvider::Bedrock),
            "the helper lands the explicit id on the named provider"
        );
        assert_eq!(repl.state.model.as_deref(), Some("my-tune"));
        let last = repl.chat.last_message().unwrap().content.clone();
        assert!(
            last.contains("my-tune"),
            "switched message names the id: {last}"
        );
    }

    // ── Grammar: `/provider <name> [model-id]` ──────────────────────────

    #[test]
    fn parse_provider_args_splits_first_token_only() {
        assert_eq!(parse_provider_args("azure"), ("azure", None));
        assert_eq!(
            parse_provider_args("azure gpt-4o-azure"),
            ("azure", Some("gpt-4o-azure".to_string()))
        );
        // Slash-qualified ids stay intact — only the first token is the slug.
        assert_eq!(
            parse_provider_args("openrouter anthropic/claude-sonnet-4"),
            ("openrouter", Some("anthropic/claude-sonnet-4".to_string()))
        );
        // Whitespace-tolerant on both sides.
        assert_eq!(
            parse_provider_args("  ollama   llama3  "),
            ("ollama", Some("llama3".to_string()))
        );
    }
}
