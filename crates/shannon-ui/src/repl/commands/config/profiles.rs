//! `/profiles` command handlers — multi-profile phase 2 (R3-2).
//!
//! `/profiles` manages the named model profiles of `providers.toml` v2
//! ([`shannon_types::provider_config::ProviderModelConfig::profiles`]): the
//! no-arg form lists them (active one marked), `use` switches the active
//! profile (persisted via [`ProviderConfigService::set_active_profile`]) and
//! re-resolves the running engine client through the same single switch path
//! the `/model` / `/provider` commands use ([`apply_model_selection`], which
//! also re-binds the declared-models registry to the new profile), `new`
//! creates an empty profile and switches to it, and `rename` / `delete`
//! round out the lifecycle. Persistence, existence checks and the
//! empty-profile refusal live in the service so the CLI and desktop surfaces
//! cannot diverge from the REPL.
//!
//! Naming transition note (R1-6 / decision ②): this command is `/profiles`
//! (plural). `/profile` remains reserved for the permission-profile command
//! during the transition and is intentionally untouched here.

use super::apply_model_selection;
use crate::repl::Repl;
use crate::{Result, widgets::ChatRole};
use shannon_core::provider_config_service::{ProfileList, ProviderConfigService};
use shannon_core::provider_resolver::{resolve_active_target, resolve_credential};

/// A parsed `/profiles` subcommand. Pure — no side effects, so it is
/// unit-tested without a `Repl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProfilesAction {
    /// `/profiles` — the dashboard.
    List,
    /// `/profiles use <name>`.
    Use(String),
    /// `/profiles new <name>`.
    New(String),
    /// `/profiles rename <old> <new>`.
    Rename { old: String, new: String },
    /// `/profiles delete <name> [--force]`.
    Delete { name: String, force: bool },
}

/// Parse `/profiles` arguments into a [`ProfilesAction`]. `Err` carries the
/// usage text (or the specific complaint) for the caller to display.
pub(crate) fn parse_profiles_action(args: &str) -> std::result::Result<ProfilesAction, String> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    match parts.as_slice() {
        [] => Ok(ProfilesAction::List),
        ["use", name] => Ok(ProfilesAction::Use((*name).to_string())),
        ["new", name] => Ok(ProfilesAction::New((*name).to_string())),
        ["rename", old, new] => Ok(ProfilesAction::Rename {
            old: (*old).to_string(),
            new: (*new).to_string(),
        }),
        // `delete` accepts an optional trailing `--force`; anything else
        // beyond the name is a usage error rather than a silent ignore.
        ["delete", rest @ ..] | ["remove", rest @ ..] => match rest {
            [name] => Ok(ProfilesAction::Delete {
                name: (*name).to_string(),
                force: false,
            }),
            [name, "--force"] => Ok(ProfilesAction::Delete {
                name: (*name).to_string(),
                force: true,
            }),
            _ => Err(usage()),
        },
        ["use"] | ["new"] => Err(format!("missing <name>.\n\n{}", usage())),
        ["rename", _] => Err(format!("missing <new> name.\n\n{}", usage())),
        _ => Err(usage()),
    }
}

/// The `/profiles` usage block. Short lines so the chat panel does not wrap
/// it into a jagged layout (same discipline as the `/connect` dashboard).
fn usage() -> String {
    "Usage:\n\
     \x20 /profiles                     list profiles (active one marked)\n\
     \x20 /profiles use <name>          switch the active profile\n\
     \x20 /profiles new <name>          create an empty profile and switch to it\n\
     \x20 /profiles rename <old> <new>  rename a profile\n\
     \x20 /profiles delete <name> [--force]\n\
     \x20                               delete a profile (--force for the active one)"
        .to_string()
}

pub(crate) fn handle_profiles(repl: &mut Repl, args: &str) -> Result<()> {
    // Fresh service = fresh on-disk snapshot (the constructor reads
    // providers.toml under the cross-process flock).
    let mut svc = ProviderConfigService::load();
    handle_profiles_with_service(repl, &mut svc, args)
}

/// Service-injected core of [`handle_profiles`] — the hermetic seam the
/// offline tests drive against a temp `providers.toml` (the production
/// entry point is a thin `ProviderConfigService::load()` wrapper).
pub(crate) fn handle_profiles_with_service(
    repl: &mut Repl,
    svc: &mut ProviderConfigService,
    args: &str,
) -> Result<()> {
    match parse_profiles_action(args) {
        Ok(action) => {
            match action {
                ProfilesAction::List => show_dashboard(repl, &svc.list_profiles()),
                ProfilesAction::Use(name) => switch_profile(repl, svc, &name)?,
                ProfilesAction::New(name) => create_profile(repl, svc, &name),
                ProfilesAction::Rename { old, new } => rename_profile(repl, svc, &old, &new),
                ProfilesAction::Delete { name, force } => delete_profile(repl, svc, &name, force)?,
            }
            Ok(())
        }
        Err(usage_msg) => {
            repl.chat.add_message(ChatRole::System, usage_msg);
            Ok(())
        }
    }
}

/// Render the profile listing (no-arg `/profiles`). Pure so the exact layout
/// is unit-testable: active profile marked with `*`, per-profile provider
/// count + active provider/model, then the usage block.
fn dashboard_lines(list: &ProfileList) -> Vec<String> {
    let mut lines = vec!["Model profiles:".to_string(), String::new()];
    if list.profiles.is_empty() {
        lines.push("  (none configured yet — everything runs on the built-in default)".to_string());
    }
    for p in &list.profiles {
        let target = if p.active_provider_id.is_empty() {
            "(no active model — /connect to add a provider)".to_string()
        } else {
            format!(
                "{} / {}",
                p.active_provider_id,
                if p.active_model_id.is_empty() {
                    "—"
                } else {
                    &p.active_model_id
                }
            )
        };
        let providers = if p.provider_count == 1 {
            "provider"
        } else {
            "providers"
        };
        let marker = if p.active { "*" } else { " " };
        lines.push(format!(
            "  {marker} {name} — {count} {providers} — {target}",
            name = p.name,
            count = p.provider_count,
        ));
    }
    lines.push(String::new());
    lines.push(usage());
    lines
}

fn show_dashboard(repl: &mut Repl, list: &ProfileList) {
    repl.chat
        .add_message(ChatRole::System, dashboard_lines(list).join("\n"));
}

/// `/profiles use <name>` — persist the switch, then re-resolve the running
/// engine client through [`apply_model_selection`] (the single switch path:
/// REPL state, engine provider/model, preferences, declared-models
/// re-registration and the StatusCard refresh all happen there), and
/// hot-swap the profile's credential so a `CredentialRef::Store` key works
/// without a restart (mirrors `/connect` step 5).
fn switch_profile(repl: &mut Repl, svc: &mut ProviderConfigService, name: &str) -> Result<()> {
    let switched = match svc.set_active_profile(name) {
        Ok(s) => s,
        Err(e) => {
            super::super::set_error(repl, &e.to_string());
            return Ok(());
        }
    };

    let mut lines = vec![format!("Switched to profile '{}'.", switched.profile)];

    // Resolve the new profile's target through the same identity bridge the
    // engine launch path uses (slug first, base_url fallback). The service
    // validated switchability, so this is Some in practice; the else branch
    // keeps the message honest if another writer raced us.
    if let Some(rt) = resolve_active_target(svc.store().config()) {
        let model_id = rt.model_id.to_string();
        let ctx_opt = apply_model_selection(
            repl,
            rt.provider.clone(),
            Some(model_id.clone()),
            None,
            false,
        )?;
        // The switched-to provider may carry a Store credential the env chain
        // cannot see — load it into the running client now (no restart).
        let key = resolve_credential(&rt.profile.credential);
        if !key.is_empty() {
            if let Some(engine) = repl.query_engine.as_mut() {
                engine.reload_credential(&key);
            }
        }
        lines.push(format!(
            "Active model: {provider} / {model} (context: {ctx})",
            provider = rt.provider,
            model = model_id,
            ctx = super::model::format_context_label(ctx_opt),
        ));
    } else {
        lines.push(
            "This profile has no active model yet — run /connect to add a provider.".to_string(),
        );
    }
    repl.chat.add_message(ChatRole::System, lines.join("\n"));
    Ok(())
}

/// `/profiles new <name>` — create an empty profile and switch to it. The
/// engine is intentionally left untouched (an empty profile has nothing to
/// resolve); the message tells the user where new `/connect`s will land.
fn create_profile(repl: &mut Repl, svc: &mut ProviderConfigService, name: &str) {
    let created = match svc.create_profile(name) {
        Ok(c) => c,
        Err(e) => {
            super::super::set_error(repl, &e.to_string());
            return;
        }
    };
    repl.chat.add_message(
        ChatRole::System,
        format!(
            "Created profile '{}' and switched to it.\n\n\
             It has no providers yet — /connect <provider> to add one \
             (it will be saved into this profile). The running engine keeps \
             its current model until the new profile has one.",
            created.name
        ),
    );
}

/// `/profiles rename <old> <new>` — contents move wholesale; when the
/// renamed profile was active, the active pointer followed it and the live
/// engine target is unchanged.
fn rename_profile(repl: &mut Repl, svc: &mut ProviderConfigService, old: &str, new: &str) {
    let renamed = match svc.rename_profile(old, new) {
        Ok(r) => r,
        Err(e) => {
            super::super::set_error(repl, &e.to_string());
            return;
        }
    };
    let mut msg = format!("Renamed profile '{}' → '{}'.", renamed.old, renamed.new);
    if renamed.was_active {
        msg.push_str("\nIt was the active profile — the active pointer followed the rename (engine target unchanged).");
    }
    repl.chat.add_message(ChatRole::System, msg);
}

/// `/profiles delete <name> [--force]` — refuse the last profile and the
/// active one (unless `--force`). When the active one was force-deleted, the
/// service falls back to "default" (or the first remaining profile); if that
/// fallback resolves, the engine is re-pointed at it through the same switch
/// path as `use`.
fn delete_profile(
    repl: &mut Repl,
    svc: &mut ProviderConfigService,
    name: &str,
    force: bool,
) -> Result<()> {
    let deleted = match svc.delete_profile(name, force) {
        Ok(d) => d,
        Err(e) => {
            super::super::set_error(repl, &e.to_string());
            return Ok(());
        }
    };

    let mut lines = vec![format!("Deleted profile '{}'.", deleted.removed)];
    if let Some(fallback) = &deleted.fallback {
        lines.push(format!("Fell back to profile '{fallback}'."));
        if let Some(rt) = resolve_active_target(svc.store().config()) {
            let model_id = rt.model_id.to_string();
            let ctx_opt = apply_model_selection(
                repl,
                rt.provider.clone(),
                Some(model_id.clone()),
                None,
                false,
            )?;
            let key = resolve_credential(&rt.profile.credential);
            if !key.is_empty() {
                if let Some(engine) = repl.query_engine.as_mut() {
                    engine.reload_credential(&key);
                }
            }
            lines.push(format!(
                "Active model: {provider} / {model} (context: {ctx})",
                provider = rt.provider,
                model = model_id,
                ctx = super::model::format_context_label(ctx_opt),
            ));
        } else {
            lines.push(
                "The fallback profile has no active model yet — run /connect to add a provider."
                    .to_string(),
            );
        }
    }
    repl.chat.add_message(ChatRole::System, lines.join("\n"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repl::commands::submit_input;
    use shannon_core::provider_config_service::ProviderConfigService;
    use shannon_types::provider_config::{
        ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind,
        ProviderModelConfig, ProviderProfile, Scope,
    };
    use std::collections::HashMap;
    use tempfile::TempDir;

    // ── parse_profiles_action (pure) ─────────────────────────────────────

    #[test]
    fn parse_empty_is_list() {
        assert_eq!(parse_profiles_action(""), Ok(ProfilesAction::List));
        assert_eq!(parse_profiles_action("   "), Ok(ProfilesAction::List));
    }

    #[test]
    fn parse_use_new_rename() {
        assert_eq!(
            parse_profiles_action("use work"),
            Ok(ProfilesAction::Use("work".to_string()))
        );
        assert_eq!(
            parse_profiles_action("new personal"),
            Ok(ProfilesAction::New("personal".to_string()))
        );
        assert_eq!(
            parse_profiles_action("rename old new"),
            Ok(ProfilesAction::Rename {
                old: "old".to_string(),
                new: "new".to_string()
            })
        );
    }

    #[test]
    fn parse_delete_with_and_without_force() {
        assert_eq!(
            parse_profiles_action("delete work"),
            Ok(ProfilesAction::Delete {
                name: "work".to_string(),
                force: false
            })
        );
        assert_eq!(
            parse_profiles_action("delete work --force"),
            Ok(ProfilesAction::Delete {
                name: "work".to_string(),
                force: true
            })
        );
        // `remove` is a convenience alias.
        assert_eq!(
            parse_profiles_action("remove work"),
            Ok(ProfilesAction::Delete {
                name: "work".to_string(),
                force: false
            })
        );
        // Junk after --force is a usage error, not a silent ignore.
        assert!(parse_profiles_action("delete work --force extra").is_err());
    }

    #[test]
    fn parse_missing_names_carry_usage() {
        for input in ["use", "new", "rename old", "bogus", "use a b"] {
            let err = parse_profiles_action(input).expect_err(input);
            assert!(
                err.contains("Usage:"),
                "'{input}' must produce the usage block, got: {err}"
            );
        }
    }

    // ── dashboard_lines (pure) ───────────────────────────────────────────

    fn summary(
        name: &str,
        active: bool,
        count: usize,
        provider: &str,
        model: &str,
    ) -> shannon_core::provider_config_service::ProfileSummary {
        shannon_core::provider_config_service::ProfileSummary {
            name: name.to_string(),
            active,
            provider_count: count,
            active_provider_id: provider.to_string(),
            active_model_id: model.to_string(),
        }
    }

    #[test]
    fn dashboard_marks_active_and_lists_targets() {
        let list = ProfileList {
            active_profile: "work".to_string(),
            profiles: vec![
                summary("default", false, 2, "anthropic", "claude-sonnet-4-20250514"),
                summary("work", true, 1, "openai", "gpt-4o"),
            ],
        };
        let text = dashboard_lines(&list).join("\n");
        assert!(text.contains("Model profiles:"), "{text}");
        // Active marker on work, not on default.
        let work_line = text.lines().find(|l| l.contains("work")).unwrap();
        assert!(
            work_line.contains('*') && work_line.contains("openai / gpt-4o"),
            "{work_line}"
        );
        let default_line = text.lines().find(|l| l.contains("default")).unwrap();
        assert!(!default_line.contains('*'), "{default_line}");
        assert!(default_line.contains("2 providers"), "{default_line}");
        // Usage block present.
        assert!(text.contains("/profiles use <name>"), "{text}");
    }

    #[test]
    fn dashboard_describes_empty_profiles() {
        let list = ProfileList {
            active_profile: "default".to_string(),
            profiles: vec![summary("fresh", true, 0, "", "")],
        };
        let text = dashboard_lines(&list).join("\n");
        let line = text.lines().find(|l| l.contains("fresh")).unwrap();
        assert!(
            line.contains("0 providers") && line.contains("no active model"),
            "{line}"
        );
    }

    #[test]
    fn dashboard_singular_provider_count() {
        let list = ProfileList {
            active_profile: "solo".to_string(),
            profiles: vec![summary("solo", true, 1, "ollama", "llama3")],
        };
        let text = dashboard_lines(&list).join("\n");
        assert!(text.contains("1 provider —"), "{text}");
    }

    // ── hermetic REPL flows (temp providers.toml — never ~/.shannon) ─────

    /// A `Repl` + a service over a temp file, seeded with two profiles:
    /// `default` (anthropic, active) and `work` (openai). `declared` adds a
    /// per-model declaration to the work profile for the re-registration
    /// assertions.
    struct Hermetic {
        _dir: TempDir,
        path: std::path::PathBuf,
    }

    fn provider_slot(id: &str, kind: ProviderKind, base_url: &str, var: &str) -> ProviderProfile {
        ProviderProfile {
            id: id.to_string(),
            kind,
            display_name: id.to_string(),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: var.to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: Default::default(),
            models: Vec::new(),
        }
    }

    fn model_profile(name: &str, provider: ProviderProfile, model: &str) -> ModelProfile {
        ModelProfile {
            name: name.to_string(),
            active_target: ActiveTarget {
                provider_id: provider.id.clone(),
                model_id: model.to_string(),
                scope: Scope::Global,
            },
            providers: vec![provider],
            auxiliary: HashMap::new(),
            credential_scope: CredentialScope::Shared,
        }
    }

    fn hermetic_service() -> (Hermetic, ProviderConfigService) {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("providers.toml");
        let anthropic = provider_slot(
            "anthropic",
            ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "SHANNON_TEST_ANTHROPIC_KEY",
        );
        let mut openai = provider_slot(
            "openai",
            ProviderKind::OpenAi,
            "https://api.openai.com/v1",
            "SHANNON_TEST_OPENAI_KEY",
        );
        openai.models = vec![shannon_types::provider_config::ModelSpec {
            id: "gpt-declared-test".to_string(),
            display_name: Some("Declared Test".to_string()),
            context_window: Some(123_456),
            max_output: None,
            cost_per_m_input: Some(1.5),
            cost_per_m_output: Some(3.0),
            capabilities: vec![],
        }];
        let cfg = ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            active_profile: String::new(),
            profiles: HashMap::from([
                (
                    "default".to_string(),
                    model_profile("default", anthropic, "claude-sonnet-4-20250514"),
                ),
                ("work".to_string(), model_profile("work", openai, "gpt-4o")),
            ]),
            gateway: Default::default(),
        };
        shannon_core::provider_config_store::save(&cfg, Some(&path)).expect("seed file");
        let svc = ProviderConfigService::load_at(&path);
        (Hermetic { _dir: dir, path }, svc)
    }

    /// Drain the last chat message (the System reply to the command).
    fn last_chat(repl: &mut Repl) -> String {
        repl.chat
            .last_message()
            .map(|m| m.content.clone())
            .unwrap_or_default()
    }

    #[test]
    fn repl_profiles_lists_with_active_marked() {
        let mut repl = Repl::new().expect("repl");
        let (_h, mut svc) = hermetic_service();
        handle_profiles_with_service(&mut repl, &mut svc, "").unwrap();
        let out = last_chat(&mut repl);
        assert!(out.contains("Model profiles:"), "{out}");
        assert!(out.contains("claude-sonnet-4-20250514"), "{out}");
        assert!(out.contains("/profiles use <name>"), "{out}");
        // default is active (marked); work is not.
        let default_line = out.lines().find(|l| l.contains("default")).unwrap();
        assert!(default_line.contains('*'), "{default_line}");
    }

    #[test]
    fn repl_profiles_use_switches_engine_state_and_persists() {
        let mut repl = Repl::new().expect("repl");
        let (h, mut svc) = hermetic_service();
        handle_profiles_with_service(&mut repl, &mut svc, "use work").unwrap();

        let out = last_chat(&mut repl);
        assert!(out.contains("Switched to profile 'work'"), "{out}");
        assert!(out.contains("gpt-4o"), "{out}");
        // REPL state followed the switch (engine client + card source).
        assert_eq!(repl.state.model.as_deref(), Some("gpt-4o"));
        assert_eq!(
            repl.state.selected_provider.as_ref().map(|p| p.to_string()),
            Some("openai".to_string())
        );
        // The switch is durable — a fresh load sees `work` as active.
        let reloaded = ProviderConfigService::load_at(&h.path);
        assert_eq!(reloaded.list_profiles().active_profile, "work");
    }

    #[test]
    fn repl_profiles_use_rebinds_declared_models_registry() {
        use shannon_core::declared_models::replace_for_provider_in;
        use shannon_core::provider_config_store::load;
        // The live re-registration inside apply_model_selection goes through
        // replace_for_provider_slug (the real on-disk file, by design); its
        // hermetic seam `replace_for_provider_in` is what this test pins —
        // the slot lookup must honor the config's ACTIVE profile, which is
        // exactly what makes a `/profiles use` switch re-bind the registry.
        shannon_core::declared_models::clear();

        let mut repl = Repl::new().expect("repl");
        let (h, mut svc) = hermetic_service();

        // Before the switch (active = default): no openai slot in the active
        // profile → registry cleared.
        let cfg = load(Some(&h.path)).expect("seeded config");
        replace_for_provider_in("openai", &cfg);
        assert!(shannon_core::declared_models::lookup("gpt-declared-test").is_none());

        // Switch (persists active_profile = work)…
        handle_profiles_with_service(&mut repl, &mut svc, "use work").unwrap();

        // …and the fresh on-disk config resolves the slot from the now-active
        // work profile: the declaration becomes authoritative.
        let cfg = load(Some(&h.path)).expect("switched config");
        replace_for_provider_in("openai", &cfg);
        let meta = shannon_core::declared_models::lookup("gpt-declared-test")
            .expect("declared meta registered from the active profile");
        assert_eq!(meta.context_window, Some(123_456));
        assert_eq!(meta.cost_per_m_input, Some(1.5));

        // Switching back to default (no declarations) clears the registry —
        // stale declarations never outlive their profile.
        handle_profiles_with_service(&mut repl, &mut svc, "use default").unwrap();
        let cfg = load(Some(&h.path)).expect("switched-back config");
        replace_for_provider_in("openai", &cfg);
        assert!(shannon_core::declared_models::lookup("gpt-declared-test").is_none());
        assert_eq!(
            ProviderConfigService::load_at(&h.path)
                .list_profiles()
                .active_profile,
            "default"
        );
    }

    #[test]
    fn repl_profiles_use_unknown_lists_available() {
        let mut repl = Repl::new().expect("repl");
        let (_h, mut svc) = hermetic_service();
        handle_profiles_with_service(&mut repl, &mut svc, "use ghost").unwrap();
        let out = last_chat(&mut repl);
        assert!(out.contains("Error:") && out.contains("ghost"), "{out}");
        assert!(
            out.contains("default") && out.contains("work"),
            "error must list available profiles: {out}"
        );
        // Nothing switched.
        assert_eq!(svc.list_profiles().active_profile, "default");
    }

    #[test]
    fn repl_profiles_new_creates_switches_and_refuses_duplicates() {
        let mut repl = Repl::new().expect("repl");
        let (h, mut svc) = hermetic_service();
        handle_profiles_with_service(&mut repl, &mut svc, "new personal").unwrap();
        let out = last_chat(&mut repl);
        assert!(out.contains("Created profile 'personal'"), "{out}");

        let reloaded = ProviderConfigService::load_at(&h.path);
        let list = reloaded.list_profiles();
        assert_eq!(list.active_profile, "personal");
        let fresh = list.profiles.iter().find(|p| p.name == "personal").unwrap();
        assert_eq!(fresh.provider_count, 0);

        // Duplicate refuses.
        handle_profiles_with_service(&mut repl, &mut svc, "new personal").unwrap();
        assert!(last_chat(&mut repl).contains("already exists"));
        // A multi-token name never parses as a name — usage is shown.
        handle_profiles_with_service(&mut repl, &mut svc, "new two words").unwrap();
        assert!(last_chat(&mut repl).contains("Usage:"));
    }

    #[test]
    fn repl_profiles_rename_moves_and_follows_active_pointer() {
        let mut repl = Repl::new().expect("repl");
        let (h, mut svc) = hermetic_service();
        handle_profiles_with_service(&mut repl, &mut svc, "use work").unwrap();
        handle_profiles_with_service(&mut repl, &mut svc, "rename work gig").unwrap();
        let out = last_chat(&mut repl);
        assert!(out.contains("'work' → 'gig'"), "{out}");
        assert!(out.contains("active pointer followed"), "{out}");

        let reloaded = ProviderConfigService::load_at(&h.path);
        let list = reloaded.list_profiles();
        assert_eq!(list.active_profile, "gig");
        assert!(list.profiles.iter().all(|p| p.name != "work"));
        // Contents moved wholesale: the renamed profile still has openai.
        let gig = list.profiles.iter().find(|p| p.name == "gig").unwrap();
        assert_eq!(gig.provider_count, 1);
        assert_eq!(gig.active_provider_id, "openai");

        // Unknown old name errors with the available list.
        handle_profiles_with_service(&mut repl, &mut svc, "rename ghost x").unwrap();
        assert!(last_chat(&mut repl).contains("not found"));
    }

    #[test]
    fn repl_profiles_delete_guards_and_force_falls_back() {
        let mut repl = Repl::new().expect("repl");
        let (h, mut svc) = hermetic_service();

        // Deleting the active profile without --force refuses (file untouched).
        handle_profiles_with_service(&mut repl, &mut svc, "delete default").unwrap();
        assert!(last_chat(&mut repl).contains("refusing to delete the active profile"));
        assert_eq!(svc.list_profiles().active_profile, "default");

        // Force-deleting the active one falls back to `work` and re-resolves
        // the engine onto it (same switch path as `use`).
        handle_profiles_with_service(&mut repl, &mut svc, "delete default --force").unwrap();
        let out = last_chat(&mut repl);
        assert!(out.contains("Deleted profile 'default'."), "{out}");
        assert!(out.contains("Fell back to profile 'work'"), "{out}");
        assert_eq!(repl.state.model.as_deref(), Some("gpt-4o"));

        // A non-active profile deletes without --force.
        handle_profiles_with_service(&mut repl, &mut svc, "new scratch").unwrap();
        handle_profiles_with_service(&mut repl, &mut svc, "delete work").unwrap();
        assert!(last_chat(&mut repl).contains("Deleted profile 'work'."));
        let list = ProviderConfigService::load_at(&h.path).list_profiles();
        assert!(list.profiles.iter().all(|p| p.name != "work"));

        // The last remaining profile can never be deleted, even with --force.
        handle_profiles_with_service(&mut repl, &mut svc, "delete scratch --force").unwrap();
        let out = last_chat(&mut repl);
        assert!(out.contains("only profile"), "{out}");
        assert_eq!(svc.list_profiles().active_profile, "scratch");
    }

    #[test]
    fn repl_profiles_dispatch_end_to_end_through_submit_input() {
        // Read-only dashboard through the real dispatch path (submit_input
        // reads the user's real providers.toml, which this test never writes).
        let mut repl = Repl::new().expect("repl");
        repl.prompt.set_input("/profiles".to_string());
        submit_input(&mut repl, None).unwrap();
        let out = last_chat(&mut repl);
        assert!(
            out.contains("Model profiles:") && out.contains("Usage:"),
            "no-arg /profiles must print the dashboard, got: {out}"
        );

        // The permission-profile transition is untouched: /profile still
        // routes to the registry (NOT this handler).
        repl.prompt.set_input("/profiles use".to_string());
        submit_input(&mut repl, None).unwrap();
        let out = last_chat(&mut repl);
        assert!(
            out.contains("Usage:"),
            "missing name must show usage: {out}"
        );
    }
}
