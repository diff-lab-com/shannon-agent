use super::super::Repl;
use crate::{Result, widgets::ChatRole};
use rust_i18n::t;
use shannon_core::model_registry;
use shannon_core::provider_resolver::resolve_model_ref;
use shannon_engine::api::LlmProvider;
use shannon_types::model_ref::ModelRef;
use shannon_types::provider_config::{ProviderTiers, TierName};
use shannon_types::recover_lock;

/// Resolve a `/model` argument into `(model_id, optional provider)` (ADR-0005
/// Phase 3).
///
/// Accepts both spellings:
/// - **Qualified** `provider/model` (e.g. `anthropic/claude-sonnet-4-…`,
///   `ollama/llama3`): the provider comes from the ref, and the model is
///   alias-expanded *within* that provider.
/// - **Bare** (legacy): an alias (`sonnet`) or literal id; the provider is set
///   only when the model is known to the catalog.
pub(crate) fn resolve_model_arg(args: &str) -> (String, Option<LlmProvider>) {
    match ModelRef::parse(args.trim()) {
        Some(mref) => {
            let r = resolve_model_ref(&mref);
            (r.model_id, Some(r.provider))
        }
        None => {
            let info = model_registry::model_info_for_alias(args.trim());
            let model_id = info
                .map(|m| m.id.to_string())
                .unwrap_or_else(|| args.trim().to_string());
            (model_id, info.map(|m| m.provider.clone()))
        }
    }
}

pub(crate) fn handle_model(repl: &mut Repl, args: &str) -> Result<()> {
    // /model --tier <name> [provider] [--save]
    if args.starts_with("--tier") {
        return handle_model_tier(repl, args);
    }

    if args.is_empty() {
        let picker = crate::widgets::select::ModelPickerWidget::new(repl.state.model.as_deref());
        repl.state.model_picker = Some(picker);
    } else {
        let (resolved_id, resolved_provider) = resolve_model_arg(args);

        // If a provider was resolved and differs from the current one, switch.
        if let Some(provider) = resolved_provider {
            if repl.state.selected_provider.as_ref() != Some(&provider) {
                repl.state.selected_provider = Some(provider);
            }
        }

        repl.state.model = Some(resolved_id.clone());
        crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
            model: repl.state.model.clone(),
            provider: repl.state.selected_provider.clone(),
            theme: Some(repl.state.theme.name.to_string()),
        });

        // Sync model to query engine and resolve real context window
        let ctx = if let Some(ref mut engine) = repl.query_engine {
            if let Some(ref provider) = repl.state.selected_provider {
                engine.set_model_for_provider(resolved_id.clone(), provider.clone());
            } else {
                engine.set_model(resolved_id.clone());
            }
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                repl.runtime.block_on(engine.pre_resolve_context());
            }));
            engine.resolved_context_window()
        } else {
            shannon_core::model_registry::context_window_for(&resolved_id)
        };
        repl.state.context_window = ctx;
        let ctx_label = if ctx >= 1_000_000 {
            format!("{}M", ctx / 1_000_000)
        } else if ctx >= 1_000 {
            format!("{}K", ctx / 1_000)
        } else {
            ctx.to_string()
        };
        let msg = format!(
            "{} (context: {ctx_label})",
            t!("commands.model.set", name = &resolved_id)
        );
        repl.chat.add_message(ChatRole::System, msg);
    }
    Ok(())
}

/// Parse a provider name string (with aliases) into an [`LlmProvider`].
fn parse_provider_name(name: &str) -> Result<LlmProvider> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "anthropic" | "claude" => Ok(LlmProvider::Anthropic),
        "openai" | "gpt" | "chatgpt" => Ok(LlmProvider::OpenAI),
        "gemini" | "google" => Ok(LlmProvider::Gemini),
        "azure" | "azure-openai" => Ok(LlmProvider::Azure),
        "bedrock" | "aws" => Ok(LlmProvider::Bedrock),
        "mistral" | "mistral-ai" => Ok(LlmProvider::Mistral),
        "deepseek" | "ds" => Ok(LlmProvider::DeepSeek),
        "groq" => Ok(LlmProvider::Groq),
        "together" | "together-ai" => Ok(LlmProvider::Together),
        "openrouter" => Ok(LlmProvider::OpenRouter),
        "cohere" => Ok(LlmProvider::Cohere),
        "fireworks" => Ok(LlmProvider::Fireworks),
        "perplexity" => Ok(LlmProvider::Perplexity),
        "xai" | "grok" => Ok(LlmProvider::Xai),
        "ai21" => Ok(LlmProvider::Ai21),
        "siliconflow" | "sf" => Ok(LlmProvider::SiliconFlow),
        "zhipu" | "zhipu-cn" | "glm" => Ok(LlmProvider::Zhipu),
        "zhipu-international" | "zhipu-intl" | "glm-intl" => Ok(LlmProvider::ZhipuInternational),
        "moonshot" | "kimi" => Ok(LlmProvider::Moonshot),
        "minimax" | "mm" => Ok(LlmProvider::Minimax),
        "dashscope" | "qwen" | "aliyun" => Ok(LlmProvider::DashScope),
        "ollama" | "local" => Ok(LlmProvider::Ollama),
        "cloudflare" | "cf" => Ok(LlmProvider::Cloudflare),
        "replicate" => Ok(LlmProvider::Replicate),
        _ => {
            let msg =
                format!("Unknown provider: {name}. Use /provider to list available providers.");
            Err(msg.into())
        }
    }
}

pub(crate) fn handle_provider(repl: &mut Repl, args: &str) -> Result<()> {
    if args.is_empty() {
        // List all providers with key status
        let providers = model_registry::all_providers();
        let mut lines = vec!["Available providers:".to_string()];
        for p in &providers {
            let has_key = !p.resolve_api_key_from_env().is_empty();
            let key_status = if p.requires_auth() {
                if has_key { "key OK" } else { "no key" }
            } else {
                "no auth"
            };
            let current = if repl.state.selected_provider.as_ref() == Some(p) {
                " *"
            } else {
                ""
            };
            lines.push(format!("  {p} — {key_status}{current}"));
        }
        lines.push(String::new());
        lines.push("* = current | Use /provider <name> to switch".to_string());
        repl.chat.add_message(ChatRole::System, lines.join("\n"));
    } else {
        // Switch to specified provider
        let provider = parse_provider_name(args.trim())?;
        let models = model_registry::models_for_provider(provider.clone());
        let default_model = models
            .first()
            .map(|m| m.id.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        repl.state.model = Some(default_model.clone());
        repl.state.selected_provider = Some(provider.clone());

        // Sync to query engine
        if let Some(ref mut engine) = repl.query_engine {
            engine.set_model_for_provider(default_model.clone(), provider.clone());
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                repl.runtime.block_on(engine.pre_resolve_context());
            }));
            repl.state.context_window = engine.resolved_context_window();
        }

        crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
            model: repl.state.model.clone(),
            provider: Some(provider),
            theme: Some(repl.state.theme.name.to_string()),
        });

        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Provider: {} | Model: {}",
                repl.state.selected_provider.as_ref().unwrap(),
                default_model
            ),
        );
    }
    Ok(())
}

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

/// Connection status label for a provider in the `/connect` dashboard
/// (ADR-0005 Phase 2). Pure — unit-tested below.
///
/// - `no auth`     provider needs no key (e.g. Ollama)
/// - `✓ connected` profile persisted AND a key is stored → works on next launch
/// - `key stored`  a key exists but `/connect` hasn't persisted a profile yet
/// - `no key`      auth required, nothing stored
fn connect_status(requires_auth: bool, connected: bool, has_key: bool) -> &'static str {
    if !requires_auth {
        "no auth"
    } else if connected && has_key {
        "✓ connected"
    } else if has_key {
        "key stored"
    } else {
        "no key"
    }
}

/// Slugs of providers that have a persisted profile in
/// `~/.shannon/providers.toml` (i.e. `/connect` was run for them).
fn connected_provider_slugs() -> std::collections::HashSet<String> {
    shannon_core::provider_config_store::load(None)
        .map(|pm| {
            pm.profiles
                .values()
                .flat_map(|p| p.providers.iter().map(|pp| pp.id.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// No-arg `/connect` dashboard: list every provider with its connection status
/// plus a one-line syntax hint. Replaces the old wall-of-text help — detailed
/// docs live at `/help connect`. Short lines so the chat panel never wraps it
/// into the jagged layout the long help string produced.
fn show_connect_dashboard(repl: &mut Repl) {
    use shannon_core::credential_manager::read_credential_value_default;
    use shannon_core::model_registry;
    use shannon_core::provider_resolver::llm_provider_id;

    let connected = connected_provider_slugs();
    let mut lines = vec![
        "Connect a provider — no env var needed.".to_string(),
        String::new(),
        "Providers:".to_string(),
    ];
    for p in model_registry::all_providers() {
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
    lines.push("Usage: /connect <provider> <api-key>   (detail: /help connect)".to_string());
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
/// — never in a config file (decision A1). The persisted v2 profile
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
            super::set_error(repl, &e.to_string());
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
fn should_prompt_for_key(requires_auth: bool, key_arg: Option<&str>, has_stored_key: bool) -> bool {
    requires_auth && key_arg.is_none() && !has_stored_key
}

/// Whether a key is already stored for `provider`'s credential service.
fn has_connect_key(provider: &LlmProvider) -> bool {
    use shannon_core::provider_resolver::build_connect_profile;
    let cp = build_connect_profile(provider.clone(), None, None);
    shannon_core::credential_manager::read_credential_value_default(&cp.service).is_some()
}

/// Persist the key (if any) + v2 profile and switch the running engine + REPL
/// state to the provider's default model. Shared by the inline-key path and the
/// wizard's submit handler.
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
    use shannon_core::credential_manager::{
        Credential, CredentialManager, read_credential_value_default,
    };
    use shannon_core::provider_config_store;
    use shannon_core::provider_resolver::build_connect_profile;

    let cp = build_connect_profile(provider, None, None);
    let display = format!("{}", cp.provider);
    let mut lines: Vec<String> = Vec::new();

    // 1. API key → credential Store (idempotent; plaintext lands only on disk
    //    at ~/.shannon/credentials/<service>.json, 0600 — never in a config).
    if let Some(new_key) = key.filter(|k| !k.is_empty()) {
        match CredentialManager::new().and_then(|mut m| {
            m.store_or_update(Credential::new(&cp.service, &cp.service, new_key))
        }) {
            Ok(_) => lines.push(format!(
                "✓ API key stored for '{display}' (service: {})",
                cp.service
            )),
            Err(e) => {
                super::set_error(repl, &format!("storing credential: {e}"));
                return Ok(());
            }
        }
    } else if read_credential_value_default(&cp.service).is_some() {
        lines.push(format!(
            "• Reusing stored key for '{display}' (service: {})",
            cp.service
        ));
    }
    // No "no key" warning branch here: the no-key case is intercepted upstream
    // (guide_to_inline_connect) before apply_connect runs; no-auth providers
    // intentionally print nothing.

    // 2. Persist the v2 profile (CredentialRef::Store) so the engine loads it
    //    on next launch — the durable, env-var-free contract.
    match provider_config_store::save(&cp.config, None) {
        Ok(path) => lines.push(format!("✓ Profile saved: {}", path.display())),
        Err(e) => {
            super::set_error(repl, &format!("saving providers.toml: {e}"));
            return Ok(());
        }
    }

    // 3. Switch the running engine + REPL state to the provider's default
    //    model (mirrors /provider). The stored key activates on next launch;
    //    the current client keeps its startup credential.
    repl.state.model = Some(cp.model_id.clone());
    repl.state.selected_provider = Some(cp.provider.clone());
    if let Some(ref mut engine) = repl.query_engine {
        engine.set_model_for_provider(cp.model_id.clone(), cp.provider.clone());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            repl.runtime.block_on(engine.pre_resolve_context());
        }));
        repl.state.context_window = engine.resolved_context_window();
    }
    crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
        model: repl.state.model.clone(),
        provider: repl.state.selected_provider.clone(),
        theme: Some(repl.state.theme.name.to_string()),
    });

    lines.push(format!(
        "✓ Switched to {} — model: {} (restart shannon to apply the new credential)",
        cp.provider, cp.model_id
    ));
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

pub(crate) fn handle_init(repl: &mut Repl) -> Result<()> {
    let mut init_info = String::new();
    let cwd = &repl.state.working_directory;

    let is_git = std::path::Path::new(cwd).join(".git").exists();
    if is_git {
        init_info.push_str("Git repository: detected\n");
    } else {
        init_info.push_str("Git repository: not found\n");
    }

    let agents_md_path = std::path::Path::new(cwd).join("AGENTS.md");
    if agents_md_path.exists() {
        init_info.push_str("AGENTS.md: already exists\n");
    } else {
        let default_content = "# Project Instructions\n\nThis file contains project-specific instructions for Shannon.\n\n## Coding Standards\n\n- Follow existing code patterns\n- Write clear, descriptive commit messages\n- Keep functions focused and concise\n\n## Project Structure\n\n- Describe your project structure here\n";
        match std::fs::write(&agents_md_path, default_content) {
            Ok(_) => init_info.push_str("AGENTS.md: created with default template\n"),
            Err(e) => init_info.push_str(&format!("AGENTS.md: failed to create ({e})\n")),
        }
    }

    init_info.push_str(&format!("Working directory: {cwd}\n"));
    repl.chat.add_message(
        ChatRole::System,
        t!("repl.project_initialized", info = init_info).to_string(),
    );
    Ok(())
}

pub(crate) fn handle_config(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::config_utils;
    use shannon_tools::config::ConfigManager;

    let mut manager = ConfigManager::new();
    if let Err(e) = manager.load() {
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.config.warning_load", error = e).to_string(),
        );
    }

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let action_str = parts.first().copied().unwrap_or("");
    let action = config_utils::parse_config_action(action_str);

    let output = match action {
        config_utils::ConfigAction::List => {
            let prefix = if action_str.is_empty() {
                None
            } else {
                parts.get(1).copied()
            };
            let keys = manager.list(prefix);
            if keys.is_empty() {
                config_utils::format_config_list()
            } else {
                let mut out = config_utils::format_config_list();
                out.push_str(&format!(
                    "\nConfig file: {}\n",
                    manager.config_path().display()
                ));
                for key in &keys {
                    let val = manager.get(key).unwrap_or(serde_json::Value::Null);
                    out.push_str(&format!("  {key} = {val}\n"));
                }
                out
            }
        }
        config_utils::ConfigAction::Get => {
            let key = parts.get(1).copied().unwrap_or("");
            if key.is_empty() {
                "Usage: /config get <key>".to_string()
            } else {
                match manager.get(key) {
                    Some(_val) => config_utils::format_config_get(key),
                    None => format!("Config key not found: {key}"),
                }
            }
        }
        config_utils::ConfigAction::Set => {
            let key = parts.get(1).copied().unwrap_or("");
            let value_str = parts.get(2).copied().unwrap_or("");
            if key.is_empty() || value_str.is_empty() {
                "Usage: /config set <key> <value>".to_string()
            } else {
                let value: serde_json::Value = if value_str == "true" {
                    serde_json::json!(true)
                } else if value_str == "false" {
                    serde_json::json!(false)
                } else if let Ok(n) = value_str.parse::<i64>() {
                    serde_json::json!(n)
                } else if let Ok(n) = value_str.parse::<f64>() {
                    serde_json::json!(n)
                } else {
                    serde_json::json!(value_str)
                };
                manager.set(key.to_string(), value.clone());
                let mut msg = match manager.save() {
                    Ok(_) => config_utils::format_config_set(key, &value.to_string()),
                    Err(e) => format!("Error: saving config: {e}"),
                };
                // ADR-0005 Phase 4: mirror engine-read flat keys into
                // ~/.shannon/config.toml (the file the engine loads on next
                // launch), so `/config set model X` actually takes effect.
                // Secrets are refused by the allowlist — they belong in
                // /credentials, never in a config file (decision A1).
                if shannon_core::config_persist::is_writable_key(key) {
                    match shannon_core::config_persist::set_global_config_key(None, key, value_str)
                    {
                        Ok(path) => msg.push_str(&format!("\n  engine config: {}", path.display())),
                        Err(e) => msg.push_str(&format!("\n  warning: config.toml: {e}")),
                    }
                }
                msg
            }
        }
        config_utils::ConfigAction::Reset => {
            let key = parts.get(1).copied().unwrap_or("");
            if key.is_empty() {
                "Usage: /config reset <key>".to_string()
            } else {
                let existed = manager.reset(key);
                let mut msg = if existed {
                    let _val = manager.get(key).unwrap_or(serde_json::Value::Null);
                    match manager.save() {
                        Ok(_) => config_utils::format_config_reset(key),
                        Err(e) => format!("Error: saving config: {e}"),
                    }
                } else {
                    config_utils::format_config_reset(key)
                };
                // ADR-0005 Phase 4: also drop the key from the engine-read
                // config.toml. Reset only deletes, so it is safe for any key.
                match shannon_core::config_persist::reset_global_config_key(None, key) {
                    Ok(true) => msg.push_str("\n  removed from config.toml."),
                    Ok(false) => {}
                    Err(e) => msg.push_str(&format!("\n  warning: config.toml: {e}")),
                }
                msg
            }
        }
        config_utils::ConfigAction::Help => config_utils::format_config_list(),
    };

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

pub(crate) fn handle_mode(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_engine::permissions::ApprovalMode;

    let trimmed = args.trim();

    if trimmed.is_empty() {
        // Show current mode and available options
        let current = {
            let query_engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(
                        ChatRole::System,
                        "Error: Query engine not available.".to_string(),
                    );
                    return Ok(());
                }
            };
            let permissions = recover_lock(query_engine.permissions().read());
            permissions.approval_mode()
        };
        let mut msg = format!("Current approval mode: {current}\n\nAvailable modes:\n");
        for name in ApprovalMode::all_names() {
            let mode = ApprovalMode::from_str_ci(name)
                .expect("from_str_ci should return valid mode for all_names()");
            let marker = if mode == current { " *" } else { "" };
            msg.push_str(&format!("  {name}{marker} — {}\n", mode.description()));
        }
        {
            repl.chat.add_message(ChatRole::System, msg);
        }
        return Ok(());
    }

    match ApprovalMode::from_str_ci(trimmed) {
        Some(mode) => {
            let query_engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(
                        ChatRole::System,
                        "Error: Query engine not available.".to_string(),
                    );
                    return Ok(());
                }
            };
            recover_lock(query_engine.permissions().write()).set_approval_mode(mode);
            repl.state.approval_mode_label = mode.short_label().to_string();
            {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Approval mode set to: {mode}\n{}", mode.description()),
                );
            }
            Ok(())
        }
        None => {
            let valid = ApprovalMode::all_names().join(", ");
            {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Unknown mode: '{trimmed}'. Valid modes: {valid}"),
                );
            }
            Ok(())
        }
    }
}

pub(crate) fn handle_context(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    if trimmed == "reload" {
        // Reload project context into the query engine
        let cwd = std::env::current_dir().unwrap_or_default();
        match shannon_core::project_instructions::load_full_context(&cwd) {
            Some(instructions) => {
                let query_engine = match repl.query_engine.as_mut() {
                    Some(e) => e,
                    None => {
                        repl.chat.add_message(
                            ChatRole::System,
                            "Error: Query engine not available.".to_string(),
                        );
                        return Ok(());
                    }
                };
                query_engine.append_system_prompt(&instructions.content);
                let files = instructions.loaded_files.join(", ");
                {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Project context reloaded. Loaded: {files}"),
                    );
                }
            }
            None => {
                repl.chat.add_message(
                        ChatRole::System,
                        "No project context found (no CLAUDE.md/AGENTS.md/GEMINI.md and not in a git repo)".to_string(),
                    );
            }
        }
        return Ok(());
    }

    if trimmed == "usage" {
        let total = repl.state.tokens_used;
        let input = repl.state.input_tokens;
        let output = repl.state.output_tokens;
        let cached = repl.state.cache_read_tokens + repl.state.cache_creation_tokens;
        let other = total.saturating_sub(input + output + cached);

        // Build a colored bar using Unicode block chars
        let bar_w = 40usize;
        let max_ctx = 200_000u64; // default context window
        let pct = if total > 0 {
            (total as f64 / max_ctx as f64).min(1.0)
        } else {
            0.0
        };
        let filled = (pct * bar_w as f64).round() as usize;

        let input_w = if total > 0 {
            (input as f64 / max_ctx as f64 * bar_w as f64).round() as usize
        } else {
            0
        };
        let output_w = if total > 0 {
            (output as f64 / max_ctx as f64 * bar_w as f64).round() as usize
        } else {
            0
        };
        let cached_w = if total > 0 {
            (cached as f64 / max_ctx as f64 * bar_w as f64).round() as usize
        } else {
            0
        };
        let other_w = filled.saturating_sub(input_w + output_w + cached_w);

        let mut bar = String::from("[");
        for _ in 0..input_w {
            bar.push('█');
        }
        for _ in 0..output_w {
            bar.push('▓');
        }
        for _ in 0..cached_w {
            bar.push('░');
        }
        for _ in 0..other_w {
            bar.push('▒');
        }
        for _ in 0..(bar_w.saturating_sub(filled)) {
            bar.push('·');
        }
        bar.push(']');

        let fmt_tok = |t: u64| -> String {
            if t < 1000 {
                format!("{t}")
            } else if t < 1_000_000 {
                format!("{:.1}k", t as f64 / 1000.0)
            } else {
                format!("{:.1}M", t as f64 / 1_000_000.0)
            }
        };

        let mut msg = String::from("Context Window Usage\n\n");
        msg.push_str(&format!("  {} {:.1}%\n\n", bar, pct * 100.0));
        msg.push_str(&format!("  █ Input:    {} tokens\n", fmt_tok(input)));
        msg.push_str(&format!("  ▓ Output:   {} tokens\n", fmt_tok(output)));
        msg.push_str(&format!("  ░ Cached:   {} tokens\n", fmt_tok(cached)));
        if other > 0 {
            msg.push_str(&format!("  ▒ Other:    {} tokens\n", fmt_tok(other)));
        }
        msg.push_str(&format!(
            "  · Free:     {} tokens\n\n",
            fmt_tok(max_ctx.saturating_sub(total))
        ));
        msg.push_str(&format!(
            "  Total used: {} / {} tokens\n",
            fmt_tok(total),
            fmt_tok(max_ctx)
        ));

        if pct > 0.8 {
            msg.push_str("\n  ⚠ Context is over 80% used. Consider /compact to free space.");
        }

        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    // Show current project context info
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut msg = String::from("Project Context:\n\n");

    // Check instruction files
    let instruction_files = ["CLAUDE.md", "AGENTS.md", "GEMINI.md"];
    let mut found_any = false;
    for filename in &instruction_files {
        let path = cwd.join(filename);
        if path.is_file() {
            found_any = true;
            msg.push_str(&format!("  {filename}: found\n"));
        } else {
            msg.push_str(&format!("  {filename}: not found\n"));
        }
    }

    // Check parent directories for instruction files
    let mut current = cwd.parent();
    while let Some(parent) = current {
        for filename in &instruction_files {
            if parent.join(filename).is_file() {
                msg.push_str(&format!("  {filename}: found in {}\n", parent.display()));
                found_any = true;
            }
        }
        current = parent.parent();
    }

    // Git context
    if let Some(git_ctx) = shannon_core::project_instructions::git_context(&cwd) {
        msg.push_str(&format!("\n{git_ctx}"));
        found_any = true;
    } else {
        msg.push_str("\nGit: not a git repository\n");
    }

    if !found_any {
        msg.push_str(
            "\nNo project context available. Create an AGENTS.md file or initialize a git repo.",
        );
    }

    msg.push_str("\nTip: Use /context reload to refresh the project context.");
    {
        repl.chat.add_message(ChatRole::System, msg);
    }
    Ok(())
}

pub(crate) fn handle_local_models(repl: &mut Repl) -> Result<()> {
    let mut output = String::from("Local Model Detection\n\n");

    // Check Ollama
    let ollama_check = std::process::Command::new("curl")
        .args([
            "-s",
            "--connect-timeout",
            "3",
            "http://localhost:11434/api/tags",
        ])
        .output();

    match ollama_check {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            if stdout.is_empty() || !result.status.success() {
                output.push_str("Ollama: not running (localhost:11434 unreachable)\n");
            } else {
                output.push_str("Ollama: running at localhost:11434\n");
                // Parse model list
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    if let Some(models) = json.get("models").and_then(|m| m.as_array()) {
                        if models.is_empty() {
                            output.push_str("  No models installed\n");
                        } else {
                            output.push_str(&format!("  Available models ({}):\n", models.len()));
                            for model in models {
                                let name = model
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("unknown");
                                let size = model
                                    .get("size")
                                    .and_then(|s| s.as_u64())
                                    .map(|b| format!("{:.1} GB", b as f64 / 1e9))
                                    .unwrap_or_default();
                                output.push_str(&format!("    - {name} ({size})\n"));
                            }
                        }
                    }
                } else {
                    output.push_str("  Could not parse model list\n");
                }
            }
        }
        Err(_) => {
            output.push_str("Ollama: not detected (curl not available or host unreachable)\n");
        }
    }

    // Check LM Studio
    let lmstudio_check = std::process::Command::new("curl")
        .args([
            "-s",
            "--connect-timeout",
            "3",
            "http://localhost:1234/v1/models",
        ])
        .output();

    match lmstudio_check {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            if stdout.is_empty() || !result.status.success() {
                output.push_str("\nLM Studio: not running (localhost:1234 unreachable)\n");
            } else {
                output.push_str("\nLM Studio: running at localhost:1234\n");
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    if let Some(models) = json.get("data").and_then(|m| m.as_array()) {
                        if models.is_empty() {
                            output.push_str("  No models loaded\n");
                        } else {
                            output.push_str(&format!("  Loaded models ({}):\n", models.len()));
                            for model in models {
                                let id = model
                                    .get("id")
                                    .and_then(|i| i.as_str())
                                    .unwrap_or("unknown");
                                output.push_str(&format!("    - {id}\n"));
                            }
                        }
                    }
                }
            }
        }
        Err(_) => {
            output.push_str("\nLM Studio: not detected\n");
        }
    }

    // Suggest usage
    output.push_str("\nTo use a local model:\n");
    output.push_str("  /model ollama/llama3\n");
    output.push_str("  /model ollama/mistral\n");
    output.push_str("  /model lmstudio/<model-id>\n");
    output.push_str(&format!(
        "\nCurrent model: {}\n",
        repl.state.model.as_deref().unwrap_or("not set")
    ));

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

/// /theme — switch color theme or list available themes.
pub(crate) fn handle_theme(repl: &mut Repl, args: &str) -> Result<()> {
    use crate::theme::Theme;

    let args = args.trim();

    if args == "pick" || args == "picker" || args == "preview" {
        let themes = Theme::available();
        let current = &repl.state.theme.name;
        let items: Vec<_> = themes
            .into_iter()
            .map(|name| {
                let label = if name == *current {
                    format!("{name} (current)")
                } else {
                    name.clone()
                };
                crate::widgets::select::SelectItem::new(label, name)
            })
            .collect();

        let picker = crate::widgets::select::FuzzyPickerWidget::new("Theme Picker".to_string())
            .with_items(items);
        repl.state.theme_picker = Some(picker);
        return Ok(());
    }

    if args.is_empty() || args == "list" {
        let current = &repl.state.theme.name;
        let available = Theme::available();
        let mut msg = String::from("Available themes:\n");
        for name in available {
            if name == *current {
                msg.push_str(&format!("  * {name} (current)\n"));
            } else {
                msg.push_str(&format!("    {name}\n"));
            }
        }
        msg.push_str("\nUsage: /theme <name>");
        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    match Theme::named(args) {
        Some(theme) => {
            let name = theme.name.clone();
            repl.renderer.set_theme(&theme);
            repl.state.theme = theme;
            crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
                model: repl.state.model.clone(),
                provider: repl.state.selected_provider.clone(),
                theme: Some(name.to_string()),
            });
            repl.chat
                .add_message(ChatRole::System, format!("Theme switched to '{name}'."));
        }
        None => {
            let available = Theme::available().join(", ");
            repl.chat.add_message(
                ChatRole::System,
                format!("Unknown theme '{args}'. Available: {available}"),
            );
        }
    }

    Ok(())
}

/// /accessibility — toggle or check accessibility mode.
pub(crate) fn handle_accessibility(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();
    match arg {
        "on" | "enable" | "true" | "1" => {
            repl.state.accessibility_mode = true;
            crate::a11y::set_enabled(true);
            repl.chat.add_message(
                ChatRole::System,
                "Accessibility mode enabled. Decorative characters replaced with plain text."
                    .to_string(),
            );
        }
        "off" | "disable" | "false" | "0" => {
            repl.state.accessibility_mode = false;
            crate::a11y::set_enabled(false);
            repl.chat
                .add_message(ChatRole::System, "Accessibility mode disabled.".to_string());
        }
        "" | "status" => {
            let state = if repl.state.accessibility_mode {
                "enabled"
            } else {
                "disabled"
            };
            repl.chat.add_message(ChatRole::System,
                format!("Accessibility mode: {state}\n\nUsage: /accessibility on|off\nAlso auto-enabled via NO_GRAPHICS or ACCESSIBILITY env vars."));
        }
        _ => {
            repl.chat.add_message(
                ChatRole::System,
                "Usage: /accessibility on|off|status".to_string(),
            );
        }
    }
    Ok(())
}

pub(crate) fn handle_terminal_setup(repl: &mut Repl) -> Result<()> {
    let mut report = String::from("Terminal Setup Check\n\n");

    // 1. Shell detection
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string());
    let shell_name = std::path::Path::new(&shell)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| shell.clone());
    report.push_str(&format!("Shell: {shell_name} ({shell})\n"));

    // 2. Terminal type
    let term = std::env::var("TERM").unwrap_or_else(|_| "not set".to_string());
    report.push_str(&format!("TERM: {term}\n"));

    // 3. Check if shannon is on PATH
    let shannon_on_path = std::process::Command::new("which")
        .arg("shannon")
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    report.push_str(&format!(
        "shannon on PATH: {}\n",
        if shannon_on_path {
            "yes"
        } else {
            "no — add shannon to your PATH"
        }
    ));

    // 4. Check for common terminal tools
    for tool in &["git", "gh", "node"] {
        let found = std::process::Command::new("which")
            .arg(tool)
            .output()
            .ok()
            .map(|o| o.status.success())
            .unwrap_or(false);
        report.push_str(&format!(
            "{tool}: {}\n",
            if found { "found" } else { "not found" }
        ));
    }

    // 5. Check shell integration markers
    // Claude Code uses SHANNON_INTEGRATION_DIR or similar env vars
    let has_integration = std::env::var("SHANNON_SHELL_INTEGRATION")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    report.push_str(&format!(
        "Shell integration: {}\n",
        if has_integration {
            "active"
        } else {
            "not detected — add `eval \"$(shannon init)\"` to your shell profile for inline diagnostics and key bindings"
        }
    ));

    // 6. Check terminal dimensions
    let (w, h) = crossterm::terminal::size().unwrap_or((0, 0));
    report.push_str(&format!("Terminal size: {w}x{h}\n"));
    if w < 80 {
        report.push_str("  ⚠ Terminal width < 80 columns — UI may be cramped\n");
    }

    // 7. Color support
    let colors = std::env::var("COLORTERM").unwrap_or_else(|_| "not set".to_string());
    report.push_str(&format!("COLORTERM: {colors}\n"));

    // 8. Key binding hint
    report.push_str("\nKey bindings:\n");
    report.push_str("  Enter      — submit input\n");
    report.push_str("  Ctrl+C     — cancel current operation\n");
    report.push_str("  Ctrl+D     — exit Shannon\n");
    report.push_str("  Tab        — autocomplete\n");
    report.push_str("  Up/Down    — navigate history\n");
    report.push_str("  Escape     — enter/exit vim normal mode\n");

    report.push_str("\nShell profile setup:\n");
    match shell_name.as_str() {
        "zsh" => report.push_str("  Add to ~/.zshrc:\n    eval \"$(shannon init zsh)\"\n"),
        "bash" => report.push_str("  Add to ~/.bashrc:\n    eval \"$(shannon init bash)\"\n"),
        "fish" => report
            .push_str("  Add to ~/.config/fish/config.fish:\n    shannon init fish | source\n"),
        other => report.push_str(&format!(
            "  Unknown shell '{other}'. Add the appropriate init line to your shell profile.\n"
        )),
    }

    repl.chat.add_message(ChatRole::System, report);
    Ok(())
}

/// Handle /color command — set prompt bar color per session
pub(crate) fn handle_color(repl: &mut Repl, args: &str) -> Result<()> {
    let color = args.trim();
    if color.is_empty() || color == "default" || color == "reset" {
        repl.state.prompt_bar_color = None;
        repl.prompt.set_border_color(None);
        repl.chat.add_message(
            ChatRole::System,
            "Prompt bar color reset to default.".to_string(),
        );
    } else {
        // Validate color by trying to parse it
        let parsed = parse_color_string(color);
        match parsed {
            Some(c) => {
                repl.state.prompt_bar_color = Some(color.to_string());
                repl.prompt.set_border_color(Some(c));
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Prompt bar color set to {color}."),
                );
            }
            None => {
                repl.chat.add_message(ChatRole::System, format!(
                    "Unknown color: \"{color}\". Use a named color (red, green, blue, ...) or hex (#ff0000), or \"default\" to reset."
                ));
            }
        }
    }
    Ok(())
}

/// Parse a color string into a ratatui Color
pub(crate) fn handle_statusline(repl: &mut Repl, args: &str) -> Result<()> {
    let cmd = args.trim();
    if cmd.is_empty() || cmd == "off" || cmd == "reset" || cmd == "default" {
        repl.state.statusline_command = None;
        repl.state.cached_statusline = None;
        repl.chat
            .add_message(ChatRole::System, "Custom statusline disabled.".to_string());
    } else {
        repl.state.statusline_command = Some(cmd.to_string());
        repl.state.cached_statusline = None;
        repl.state.statusline_last_update = None;
        repl.chat
            .add_message(ChatRole::System, format!("Custom statusline set to: {cmd}"));
    }
    Ok(())
}

fn parse_color_string(s: &str) -> Option<ratatui::style::Color> {
    use ratatui::style::Color;
    let lower = s.to_lowercase();
    match lower.as_str() {
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "blue" => Some(Color::Blue),
        "yellow" => Some(Color::Yellow),
        "magenta" | "purple" | "pink" => Some(Color::Magenta),
        "cyan" | "teal" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "dark_grey" | "darkgrey" => Some(Color::DarkGray),
        "lightred" | "light_red" => Some(Color::LightRed),
        "lightgreen" | "light_green" => Some(Color::LightGreen),
        "lightblue" | "light_blue" => Some(Color::LightBlue),
        "lightyellow" | "light_yellow" => Some(Color::LightYellow),
        "lightmagenta" | "light_magenta" => Some(Color::LightMagenta),
        "lightcyan" | "light_cyan" => Some(Color::LightCyan),
        "black" => Some(Color::Black),
        _ => {
            // Try hex color
            let hex = s.trim_start_matches('#');
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color::Rgb(r, g, b))
            } else {
                None
            }
        }
    }
}

pub(crate) fn handle_lang(repl: &mut Repl, args: &str) -> Result<()> {
    let supported = ["en", "zh", "hi", "es", "fr", "ar", "bn", "pt", "ru", "ja"];
    let input = args.trim();

    if input.is_empty() {
        let current = shannon_core::i18n::current_locale();
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Current language: {current}\n\nUsage: /lang <code>\nSupported: {}",
                supported.join(", ")
            ),
        );
        return Ok(());
    }

    let lang = input.to_lowercase();
    if supported.contains(&lang.as_str()) {
        shannon_core::i18n::set_locale(&lang);
        // Refresh status bar to reflect the new language immediately
        repl.state.status = t!("status.ready").to_string();
        let lang_names = [
            ("en", "English"),
            ("zh", "中文"),
            ("hi", "हिन्दी"),
            ("es", "Español"),
            ("fr", "Français"),
            ("ar", "العربية"),
            ("bn", "বাংলা"),
            ("pt", "Português"),
            ("ru", "Русский"),
            ("ja", "日本語"),
        ];
        let native_name = lang_names
            .iter()
            .find(|(c, _)| *c == lang)
            .map(|(_, n)| *n)
            .unwrap_or(&lang);
        repl.chat.add_message(
            ChatRole::System,
            format!("Language: {native_name} ({lang})"),
        );
    } else {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Unsupported language: {lang}\nSupported: {}",
                supported.join(", ")
            ),
        );
    }
    Ok(())
}

/// Handle /model --tier <name> [provider] [--save] — resolve a tier to a
/// concrete model id for the chosen (or current) provider, then switch the
/// running engine + REPL state to it. Mirrors the bare-id branch above but
/// goes through `model_registry::resolve_tier` so catalog overrides from
/// `~/.shannon/providers.toml` (Task 17) win over the static catalog.
fn handle_model_tier(repl: &mut Repl, args: &str) -> Result<()> {
    // Accept "--tier X", "--tier X provider", "--tier X provider --save".
    // First token must be either "--tier" or "--tier=<name>".
    let parts: Vec<&str> = args.split_whitespace().collect();
    let first = parts.first().copied().unwrap_or("");
    let (tier_idx, tier_str) = if first == "--tier" {
        let name = parts.get(1).copied().unwrap_or("");
        (0usize, name)
    } else if let Some(rest) = first.strip_prefix("--tier=") {
        (0usize, rest)
    } else {
        // Args did not start with --tier; the caller already filtered these.
        return Err("internal: handle_model_tier called without --tier prefix".into());
    };

    let tier = TierName::from_user_input(tier_str).ok_or_else(|| {
        format!(
            "Unknown tier '{}'. Try one of: {}",
            tier_str,
            TierName::suggestions().join(", ")
        )
    })?;
    if matches!(tier, TierName::Auto) {
        return Err(
            "--tier auto is reserved for a future spec; explicit tiers only for now".into(),
        );
    }

    // Optional provider argument: the next non-`--save` token after `--tier <name>`.
    let explicit_provider_str = parts.get(tier_idx + 2).copied();
    let save = parts.iter().any(|p| *p == "--save");
    let provider = match explicit_provider_str {
        Some(p) => parse_provider_name(p)?,
        None => repl
            .state
            .selected_provider
            .clone()
            .ok_or_else(|| {
                "No provider selected; specify one: /model --tier <tier> <provider>".to_string()
            })?,
    };

    let profile_tiers = load_provider_tiers(&provider);
    let model_id = shannon_core::model_registry::resolve_tier(
        tier_str, &provider, &profile_tiers,
    )
    .ok_or_else(|| {
        format!(
            "No model found for tier={} provider={}",
            tier.canonical(),
            provider
        )
    })?;

    let prev_model = repl.state.model.clone();
    let prev_provider = repl.state.selected_provider.clone();
    repl.state.model = Some(model_id.clone());
    repl.state.selected_provider = Some(provider.clone());

    // Sync to the engine (mirrors the bare-id branch above).
    if let Some(ref mut engine) = repl.query_engine {
        engine.set_model_for_provider(model_id.clone(), provider.clone());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            repl.runtime.block_on(engine.pre_resolve_context());
        }));
        repl.state.context_window = engine.resolved_context_window();
    } else {
        repl.state.context_window =
            shannon_core::model_registry::context_window_for(&model_id);
    }

    crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
        model: repl.state.model.clone(),
        provider: repl.state.selected_provider.clone(),
        theme: Some(repl.state.theme.name.to_string()),
    });

    if save {
        // Rollback state on failure so a bad write doesn't leave the REPL
        // pointing at an unpinned tier.
        if let Err(e) = persist_model_to_providers_toml(&provider, &model_id, tier) {
            repl.state.model = prev_model;
            repl.state.selected_provider = prev_provider;
            return Err(e);
        }
    }

    let ctx_label = if repl.state.context_window >= 1_000_000 {
        format!("{}M", repl.state.context_window / 1_000_000)
    } else if repl.state.context_window >= 1_000 {
        format!("{}K", repl.state.context_window / 1_000)
    } else {
        repl.state.context_window.to_string()
    };
    let msg = format!(
        "{} tier={} (context: {ctx_label})",
        t!("commands.model.set", name = &model_id),
        tier.canonical()
    );
    repl.chat.add_message(ChatRole::System, msg);
    Ok(())
}

/// Load the persisted per-tier model overrides for a provider from
/// `~/.shannon/providers.toml`. Stub for Task 17 — returns the default
/// (empty) `ProviderTiers` so `resolve_tier` falls back to the catalog.
fn load_provider_tiers(_provider: &LlmProvider) -> ProviderTiers {
    // TODO: load from ~/.shannon/providers.toml (Task 17)
    ProviderTiers::default()
}

/// Persist the resolved (provider, tier) → model-id mapping back into
/// `~/.shannon/providers.toml`. Stub for Task 17 — currently a no-op so
/// `--save` is accepted without crashing; the real write lands in Task 17.
fn persist_model_to_providers_toml(
    _provider: &LlmProvider,
    _model_id: &str,
    _tier: TierName,
) -> Result<()> {
    // Implemented in Task 17
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_provider_name_aliases() {
        assert!(matches!(
            parse_provider_name("claude"),
            Ok(LlmProvider::Anthropic)
        ));
        assert!(matches!(
            parse_provider_name("gpt"),
            Ok(LlmProvider::OpenAI)
        ));
        assert!(matches!(
            parse_provider_name("ds"),
            Ok(LlmProvider::DeepSeek)
        ));
        assert!(matches!(
            parse_provider_name("local"),
            Ok(LlmProvider::Ollama)
        ));
        assert!(matches!(parse_provider_name("grok"), Ok(LlmProvider::Xai)));
        assert!(matches!(parse_provider_name("glm"), Ok(LlmProvider::Zhipu)));
    }

    #[test]
    fn parse_provider_name_case_insensitive() {
        assert!(matches!(
            parse_provider_name("ANTHROPIC"),
            Ok(LlmProvider::Anthropic)
        ));
        assert!(matches!(
            parse_provider_name("OpenAI"),
            Ok(LlmProvider::OpenAI)
        ));
    }

    #[test]
    fn parse_provider_name_unknown() {
        assert!(parse_provider_name("unknown_provider").is_err());
    }

    // ── parse_connect_args (ADR-0005 Phase 4, /connect) ─────────────────

    #[test]
    fn parse_connect_args_empty_is_none() {
        assert!(parse_connect_args("").is_none());
        assert!(parse_connect_args("   ").is_none());
        assert!(parse_connect_args("\t\n").is_none());
    }

    #[test]
    fn parse_connect_args_provider_only() {
        let p = parse_connect_args("anthropic").expect("some");
        assert_eq!(p.provider_arg, "anthropic");
        assert!(p.key_arg.is_none());
    }

    #[test]
    fn parse_connect_args_provider_and_key() {
        let p = parse_connect_args("anthropic sk-ant-api03-xxx").expect("some");
        assert_eq!(p.provider_arg, "anthropic");
        assert_eq!(p.key_arg, Some("sk-ant-api03-xxx"));
    }

    #[test]
    fn parse_connect_args_trims_surrounding_whitespace() {
        let p = parse_connect_args("   openai   sk-test123   ").expect("some");
        assert_eq!(p.provider_arg, "openai");
        assert_eq!(p.key_arg, Some("sk-test123"));
    }

    #[test]
    fn parse_connect_args_blank_key_is_none() {
        // Trailing whitespace but no key → no key.
        let p = parse_connect_args("ollama   ").expect("some");
        assert_eq!(p.provider_arg, "ollama");
        assert!(p.key_arg.is_none());
    }

    // ── connect_status (ADR-0005 Phase 2, /connect dashboard) ───────────

    #[test]
    fn connect_status_no_auth_provider() {
        // Ollama-style: never needs a key, regardless of connection/key state.
        assert_eq!(connect_status(false, false, false), "no auth");
        assert_eq!(connect_status(false, true, true), "no auth");
    }

    #[test]
    fn connect_status_authed_fully_connected() {
        // Profile persisted + key stored → fully wired.
        assert_eq!(connect_status(true, true, true), "✓ connected");
    }

    #[test]
    fn connect_status_key_but_no_profile() {
        // Key exists in the store but /connect hasn't persisted a profile.
        assert_eq!(connect_status(true, false, true), "key stored");
    }

    #[test]
    fn connect_status_profile_but_no_key() {
        // Stale profile with no key → treat as "no key" (not functional).
        assert_eq!(connect_status(true, true, false), "no key");
    }

    #[test]
    fn connect_status_nothing_stored() {
        assert_eq!(connect_status(true, false, false), "no key");
    }

    // ── should_prompt_for_key (ADR-0005 Phase 4, /connect wizard) ─────────

    #[test]
    fn should_prompt_for_key_when_auth_no_key_nothing_stored() {
        // The one case the wizard exists for: needs a key, none given, none stored.
        assert!(should_prompt_for_key(true, None, false));
    }

    #[test]
    fn should_prompt_for_key_not_when_inline_key_given() {
        assert!(!should_prompt_for_key(true, Some("sk-x"), false));
    }

    #[test]
    fn should_prompt_for_key_not_when_key_already_stored() {
        // Reconnect flow: key on disk → no need to ask again.
        assert!(!should_prompt_for_key(true, None, true));
    }

    #[test]
    fn should_prompt_for_key_not_for_no_auth_provider() {
        // Ollama-style providers never prompt, regardless of stored state.
        assert!(!should_prompt_for_key(false, None, false));
        assert!(!should_prompt_for_key(false, None, true));
    }

    #[test]
    fn parse_color_string_named() {
        use ratatui::style::Color;
        assert_eq!(parse_color_string("red"), Some(Color::Red));
        assert_eq!(parse_color_string("purple"), Some(Color::Magenta));
        assert_eq!(parse_color_string("grey"), Some(Color::Gray));
        assert_eq!(parse_color_string("light_blue"), Some(Color::LightBlue));
    }

    #[test]
    fn parse_color_string_hex() {
        use ratatui::style::Color;
        assert_eq!(parse_color_string("#ff0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color_string("#00ff00"), Some(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn parse_color_string_invalid() {
        assert_eq!(parse_color_string("notacolor"), None);
        assert_eq!(parse_color_string("#xyz"), None);
    }

    // ── resolve_model_arg (ADR-0005 Phase 3) ───────────────────────────

    #[test]
    fn resolve_model_arg_qualified_full_id() {
        let (id, provider) = resolve_model_arg("anthropic/claude-sonnet-4-20250514");
        assert_eq!(id, "claude-sonnet-4-20250514");
        assert_eq!(provider, Some(LlmProvider::Anthropic));
    }

    #[test]
    fn resolve_model_arg_qualified_alias_expands_within_provider() {
        let (id, provider) = resolve_model_arg("anthropic/sonnet");
        assert_ne!(id, "sonnet");
        assert!(id.starts_with("claude-"), "got {id}");
        assert_eq!(provider, Some(LlmProvider::Anthropic));
    }

    #[test]
    fn resolve_model_arg_qualified_unknown_model_kept_and_provider_set() {
        let (id, provider) = resolve_model_arg("ollama/llama3");
        assert_eq!(id, "llama3");
        assert_eq!(provider, Some(LlmProvider::Ollama));
    }

    #[test]
    fn resolve_model_arg_bare_alias_resolves() {
        let (id, provider) = resolve_model_arg("sonnet");
        assert!(id.starts_with("claude-sonnet"), "got {id}");
        assert_eq!(provider, Some(LlmProvider::Anthropic));
    }

    #[test]
    fn resolve_model_arg_bare_full_id() {
        let (id, provider) = resolve_model_arg("claude-sonnet-4-20250514");
        assert_eq!(id, "claude-sonnet-4-20250514");
        assert_eq!(provider, Some(LlmProvider::Anthropic));
    }

    #[test]
    fn resolve_model_arg_bare_unknown_no_provider() {
        // A bare id not in the catalog: kept as-is, no provider inferred.
        let (id, provider) = resolve_model_arg("llama3");
        assert_eq!(id, "llama3");
        assert!(provider.is_none());
    }

    #[test]
    fn resolve_model_arg_trims_whitespace() {
        let (id, provider) = resolve_model_arg("  anthropic/claude-sonnet-4-20250514  ");
        assert_eq!(id, "claude-sonnet-4-20250514");
        assert_eq!(provider, Some(LlmProvider::Anthropic));
    }
}
