//! Shannon Code desktop application entry point.
//!
//! Uses Tauri v2 to wrap the Shannon AI assistant in a native desktop window
//! with a web-based chat UI. The Rust backend handles LLM communication,
//! tool execution, and state management via Tauri IPC commands.

#[cfg(feature = "tauri")]
fn main() {
    use shannon_desktop::commands;
    use shannon_desktop::commands_agents;
    use shannon_desktop::commands_chat;
    use shannon_desktop::commands_mcp;
    use shannon_desktop::commands_plugins;
    use shannon_desktop::commands_sessions;
    use shannon_desktop::extensions_commands;
    use tauri::{Emitter, Listener, Manager};
    use tauri::{
        menu::{MenuBuilder, MenuItemBuilder},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };
    use tauri_plugin_updater::UpdaterExt;

    // E5: tracing-subscriber with JSON exporter for offline performance
    // analysis. SHANNON_LOG_FORMAT=json → newline-delimited JSON to stderr;
    // any other value (or unset) → pretty human-readable output.
    let log_format = std::env::var("SHANNON_LOG_FORMAT").unwrap_or_default();
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,shannon_desktop=debug"));
    if log_format.eq_ignore_ascii_case("json") {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .json()
            .with_target(true)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .init();
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::send_message,
            commands_chat::get_conversation,
            commands_chat::list_models,
            commands_chat::get_status,
            commands_chat::cancel_query,
            commands_chat::list_tools,
            commands::configure,
            commands::switch_provider,
            commands::get_config,
            commands_sessions::new_session,
            commands_sessions::list_sessions,
            commands_sessions::search_sessions,
            commands_sessions::load_session,
            commands_sessions::export_session,
            commands::save_text_file,
            commands_sessions::switch_session,
            commands_sessions::set_session_working_dir,
            commands_sessions::delete_session,
            commands_sessions::rename_session,
            commands_sessions::duplicate_session,
            commands_sessions::branch_session,
            commands::request_permission,
            commands::respond_permission,
            commands::get_file_diff,
            commands::apply_diff,
            commands_mcp::add_mcp_server,
            commands_mcp::remove_mcp_server,
            commands_mcp::restart_mcp_server,
            commands_mcp::get_mcp_server_config,
            commands_mcp::list_mcp_servers,
            commands_mcp::list_skills,
            commands_mcp::get_skill_detail,
            commands_mcp::list_installed_addons,
            // Extensions hub P2 — MCP installers (see extensions_commands.rs)
            extensions_commands::list_featured_vendors,
            extensions_commands::list_mcp_registry_servers,
            extensions_commands::featured_vendor_to_entry,
            extensions_commands::install_mcp_stdio,
            extensions_commands::install_mcp_mcpb,
            extensions_commands::install_mcp_oauth_authorize_url,
            extensions_commands::install_mcp_oauth_complete,
            extensions_commands::install_mcp_oauth_loopback,
            extensions_commands::uninstall_mcp_server,
            // Extensions hub P3 — Skills catalog + installer
            extensions_commands::list_skill_catalog,
            extensions_commands::install_skill_from_repo,
            extensions_commands::install_native_skill,
            extensions_commands::list_installed_skill_plugins,
            extensions_commands::uninstall_skill_plugin,
            // Extensions hub P4 — Agents catalog + installer
            extensions_commands::list_agent_catalog,
            extensions_commands::install_agent_from_repo,
            extensions_commands::install_native_agent,
            extensions_commands::list_installed_agent_plugins,
            extensions_commands::uninstall_agent_plugin,
            // Extensions hub P5 — Native data sources (Obsidian + Email IMAP)
            extensions_commands::list_data_source_catalog,
            extensions_commands::list_data_source_adapters,
            extensions_commands::install_data_source,
            extensions_commands::list_installed_data_sources,
            extensions_commands::uninstall_data_source,
            extensions_commands::read_data_source_config,
            extensions_commands::query_data_source,
            // Extensions hub P6 — Security hardening
            extensions_commands::scan_prompt_injection,
            extensions_commands::scan_prompt_injection_with_readme,
            extensions_commands::verify_signature,
            extensions_commands::report_catalog_entry,
            extensions_commands::list_catalog_reports,
            extensions_commands::clear_catalog_report,
            // Plugin management (A.3)
            commands_plugins::list_plugins,
            commands_plugins::install_plugin,
            commands_plugins::install_plugin_from_git,
            commands_plugins::uninstall_plugin,
            commands_plugins::enable_plugin,
            commands_plugins::disable_plugin,
            commands_plugins::update_plugin,
            commands_plugins::list_plugin_marketplace,
            commands_plugins::list_catalog_upstreams,
            commands::start_background_task,
            commands::get_background_tasks,
            commands::cancel_background_task,
            commands_agents::list_agents,
            commands_agents::list_agent_definitions,
            commands_agents::create_agent_definition,
            commands_agents::delete_agent_definition,
            // Inter-agent message history (Phase D C3)
            commands_agents::list_agent_messages,
            commands_agents::list_agent_message_teams,
            commands_agents::record_agent_message,
            commands::list_tasks,
            commands::update_task,
            commands::get_file_tree,
            commands::get_working_dir_info,
            // Scheduled tasks, triage, history, triggered routines (Sprint 2)
            shannon_desktop::scheduled_commands::list_scheduled_tasks,
            shannon_desktop::scheduled_commands::create_scheduled_task,
            shannon_desktop::scheduled_commands::update_scheduled_task,
            shannon_desktop::scheduled_commands::delete_scheduled_task,
            shannon_desktop::scheduled_commands::toggle_scheduled_task,
            shannon_desktop::scheduled_commands::trigger_task_now,
            shannon_desktop::scheduled_commands::preview_cron,
            shannon_desktop::scheduled_commands::list_triage_items,
            shannon_desktop::scheduled_commands::mark_triage_read,
            shannon_desktop::scheduled_commands::archive_triage_item,
            shannon_desktop::scheduled_commands::get_triage_stats,
            shannon_desktop::scheduled_commands::list_task_executions,
            shannon_desktop::scheduled_commands::get_execution_detail,
            shannon_desktop::scheduled_commands::list_triggered_routines,
            shannon_desktop::scheduled_commands::toggle_triggered_routine,
            shannon_desktop::scheduled_commands::create_triggered_routine,
            shannon_desktop::scheduled_commands::get_opc_metrics,
            // Automation: hook-event catalog + custom permission profiles
            shannon_desktop::automation_commands::list_hook_events,
            shannon_desktop::automation_commands::list_permission_profiles,
            shannon_desktop::automation_commands::save_custom_profile,
            shannon_desktop::automation_commands::delete_custom_profile,
            shannon_desktop::lsp_commands::lsp_code_actions,
            shannon_desktop::lsp_commands::apply_code_action,
            shannon_desktop::lsp_commands::read_source_file,
            shannon_desktop::lsp_commands::run_file_diagnostics,
            // Worktree management (B9)
            shannon_desktop::scheduled_commands::create_task_worktree,
            shannon_desktop::scheduled_commands::list_task_worktrees,
            shannon_desktop::scheduled_commands::remove_task_worktree,
            shannon_desktop::scheduled_commands::prune_task_worktrees,
            // Onboarding seed (#75) — first-run sample tasks
            commands::seed_sample_data,
            // P3 notifications — native OS notification bridge
            commands::send_notification,
            commands::get_webhook_config,
            commands::save_webhook_config,
            commands::clear_webhook_config,
            // P5 Phase 1 — inbound notifications (Slack + Telegram) config storage
            commands::get_inbound_config,
            commands::save_inbound_config,
            commands::clear_inbound_config,
            // P5 Phase 2 — inbound listener supervisor
            commands::get_inbound_listener_status,
            commands::stop_inbound_listener,
            // P0-c — billing demo data (UI shows "Demo mode" banner)
            commands::get_billing_plan,
            commands::get_cost_history,
            commands::get_billing_history,
        ])
        .setup(|app| {
            let mut state = commands::AppState::new();
            state.attach_notification_handler(app.handle().clone());
            app.manage(state);

            // P5 Phase 2 — auto-start inbound listener if config already exists.
            let app_handle = app.handle().clone();
            let state_ref: tauri::State<'_, commands::AppState> = app.state();
            tauri::async_runtime::block_on(async move {
                commands::bootstrap_inbound_listener(&*state_ref, &app_handle).await;
            });

            // Bundle A — Click-to-foreground: when a Shannon notification is
            // clicked, bring the main window to the foreground. On macOS and
            // Windows the OS already focuses the app automatically (native
            // behavior for notifications from a registered bundle identifier);
            // this listener ensures explicit focus when the event fires, which
            // covers Linux DEs that emit click events and any future Tauri
            // plugin versions that route desktop clicks here.
            let click_handle = app.handle().clone();
            let _ = app.listen("notification-clicked", move |_event| {
                use tauri::Manager;
                if let Some(webview_window) = click_handle.get_webview_window("main") {
                    let _ = webview_window.unminimize();
                    let _ = webview_window.show();
                    let _ = webview_window.set_focus();
                }
            });

            // Register global shortcut handlers
            use tauri_plugin_global_shortcut::GlobalShortcutExt;

            // Show/hide window shortcut
            let _ = app
                .global_shortcut()
                .on_shortcut("show-hide", |app, _shortcut_id, _| {
                    if let Some(webview_window) = app.get_webview_window("main") {
                        let _ = if webview_window.is_visible().unwrap_or(false) {
                            webview_window.hide()
                        } else {
                            webview_window.show()
                        };
                        let _ = webview_window.set_focus();
                    }
                });

            // New session shortcut
            let _ = app
                .global_shortcut()
                .on_shortcut("new-session", |app, _shortcut_id, _| {
                    let _ = app.emit("new-session", ());
                });

            // Focus input shortcut
            let _ = app
                .global_shortcut()
                .on_shortcut("focus-input", |app, _shortcut_id, _| {
                    let _ = app.emit("focus-input", ());
                });

            // Listen for check-updates events from frontend
            let handle = app.handle().clone();
            let _ = app.listen("check-updates", move |_event| {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(Some(update_info)) = handle.updater()?.check().await {
                        let payload = serde_json::json!({
                            "version": update_info.version,
                            "date": update_info.date.map(|d| d.to_string()),
                            "body": update_info.body
                        });
                        let _ = handle.emit("update-available", payload);
                    }
                    Ok::<(), tauri_plugin_updater::Error>(())
                });
            });

            // System tray configuration.
            //
            // Audit #25: the status line and tooltip previously hardcoded
            // `anthropic / claude-sonnet-4-6`. They are now built from the
            // current desktop config, and a background task refreshes the tray
            // whenever the provider/model changes (covers both `configure` and
            // `switch_provider`).
            let initial_label = tray_status_label(&shannon_desktop::config::load_config());
            let show_item = MenuItemBuilder::with_id("show", "Show Shannon").build(app)?;
            let new_session_item =
                MenuItemBuilder::with_id("new-session", "New Session").build(app)?;
            let check_updates_item =
                MenuItemBuilder::with_id("check-updates", "Check for Updates").build(app)?;
            let status_item = MenuItemBuilder::with_id("status", initial_label.clone())
                .enabled(false)
                .build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

            let menu = MenuBuilder::new(app)
                .items(&[
                    &status_item,
                    &show_item,
                    &new_session_item,
                    &check_updates_item,
                    &quit_item,
                ])
                .build()?;

            let _tray = TrayIconBuilder::with_id(TRAY_ID)
                .tooltip(format!("Shannon AI Assistant — {initial_label}"))
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(webview_window) = app.get_webview_window("main") {
                            let _ = webview_window.unminimize();
                            let _ = webview_window.show();
                            let _ = webview_window.set_focus();
                        }
                    }
                    "new-session" => {
                        // Trigger new session via event
                        let _ = app.emit("new-session", ());
                    }
                    "check-updates" => {
                        // Trigger update check via event
                        let _ = app.emit("check-updates", ());
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => (),
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(webview_window) = app.get_webview_window("main") {
                            let _ = webview_window.unminimize();
                            let _ = webview_window.show();
                            let _ = webview_window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Audit #25: refresh the tray menu + tooltip when the provider or
            // model changes. We poll the persisted config every 2s (cheap:
            // a small JSON read from `~/.shannon/desktop/config.json`)
            // because `switch_provider` does not emit a `config-updated`
            // event we could listen for. Listening to `config-updated`
            // (which `configure(...)` emits) would miss the `switch_provider`
            // path, so polling the persisted file is the single reliable seam
            // that covers both code paths.
            let refresh_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut last_label = initial_label;
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    let label = tray_status_label(&shannon_desktop::config::load_config());
                    if label == last_label {
                        continue;
                    }
                    last_label = label.clone();
                    // Rebuild the menu with the new status line and update the
                    // tooltip. If the tray lookup or build fails we log and
                    // try again on the next tick.
                    if let Err(e) = rebuild_tray_menu(&refresh_handle, &label) {
                        tracing::warn!(error = %e, "tray refresh: failed to rebuild menu");
                    }
                }
            });

            // Auto-update check on startup
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(Some(update_info)) = handle.updater()?.check().await {
                    // Emit update-available event for frontend
                    let payload = serde_json::json!({
                        "version": update_info.version,
                        "date": update_info.date.map(|d| d.to_string()),
                        "body": update_info.body
                    });
                    let _ = handle.emit("update-available", payload);
                } else {
                    tracing::info!("No updates available or update check failed");
                }
                Ok::<(), tauri_plugin_updater::Error>(())
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(not(feature = "tauri"))]
fn main() {
    eprintln!("Shannon Desktop requires the `tauri` feature.");
    eprintln!("Build with: cargo build -p shannon-desktop --features tauri");
    std::process::exit(1);
}

// ---------------------------------------------------------------------------
// System-tray helpers (audit #25)
// ---------------------------------------------------------------------------

/// Stable identifier for the Shannon tray icon so we can look it up with
/// `app.tray_by_id(TRAY_ID)` when refreshing its menu/tooltip.
#[cfg(feature = "tauri")]
const TRAY_ID: &str = "main";

/// Build the human-readable status label shown in the tray menu and tooltip.
/// Falls back to sane defaults when the config is missing fields.
///
/// Format: `Status: <provider> / <model>`. Provider defaults to `anthropic`,
/// model to `claude-sonnet-4-6` (the prior hardcoded value) — only used if the
/// config genuinely has no value yet.
#[cfg(feature = "tauri")]
fn tray_status_label(cfg: &shannon_desktop::config::DesktopConfig) -> String {
    let provider = cfg.provider.as_deref().unwrap_or("anthropic");
    let model = cfg.model.as_deref().unwrap_or("claude-sonnet-4-6");
    format!("Status: {provider} / {model}")
}

/// Rebuild the tray's menu and tooltip with an updated status label. Looks up
/// the tray by [`TRAY_ID`]; returns an error if the tray is gone (e.g. the app
/// is shutting down).
#[cfg(feature = "tauri")]
fn rebuild_tray_menu(
    app: &tauri::AppHandle,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIcon;

    let tray: TrayIcon = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| "tray icon not found".to_string())?;

    let show_item = MenuItemBuilder::with_id("show", "Show Shannon").build(app)?;
    let new_session_item = MenuItemBuilder::with_id("new-session", "New Session").build(app)?;
    let check_updates_item =
        MenuItemBuilder::with_id("check-updates", "Check for Updates").build(app)?;
    let status_item = MenuItemBuilder::with_id("status", label)
        .enabled(false)
        .build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[
            &status_item,
            &show_item,
            &new_session_item,
            &check_updates_item,
            &quit_item,
        ])
        .build()?;

    tray.set_menu(Some(menu))?;
    tray.set_tooltip(Some(format!("Shannon AI Assistant — {label}")))?;
    Ok(())
}
