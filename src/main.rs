//! Shannon Code desktop application entry point.
//!
//! Uses Tauri v2 to wrap the Shannon AI assistant in a native desktop window
//! with a web-based chat UI. The Rust backend handles LLM communication,
//! tool execution, and state management via Tauri IPC commands.

#[cfg(feature = "tauri")]
fn main() {
    use shannon_desktop::commands;
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
        .invoke_handler(tauri::generate_handler![
            commands::send_message,
            commands::get_conversation,
            commands::list_models,
            commands::get_status,
            commands::cancel_query,
            commands::list_tools,
            commands::configure,
            commands::switch_provider,
            commands::get_config,
            commands::new_session,
            commands::list_sessions,
            commands::search_sessions,
            commands::load_session,
            commands::export_session,
            commands::save_text_file,
            commands::switch_session,
            commands::delete_session,
            commands::rename_session,
            commands::duplicate_session,
            commands::request_permission,
            commands::respond_permission,
            commands::get_file_diff,
            commands::apply_diff,
            commands::add_mcp_server,
            commands::remove_mcp_server,
            commands::restart_mcp_server,
            commands::get_mcp_server_config,
            commands::list_mcp_servers,
            commands::list_skills,
            commands::get_skill_detail,
            commands::list_installed_addons,
            // Extensions hub P2 — MCP installers (see extensions_commands.rs)
            extensions_commands::list_featured_vendors,
            extensions_commands::list_mcp_registry_servers,
            extensions_commands::featured_vendor_to_entry,
            extensions_commands::install_mcp_stdio,
            extensions_commands::install_mcp_mcpb,
            extensions_commands::install_mcp_oauth_authorize_url,
            extensions_commands::install_mcp_oauth_complete,
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
            // Plugin management (A.3)
            commands::list_plugins,
            commands::install_plugin,
            commands::install_plugin_from_git,
            commands::uninstall_plugin,
            commands::enable_plugin,
            commands::disable_plugin,
            commands::update_plugin,
            commands::list_plugin_marketplace,
            commands::start_background_task,
            commands::get_background_tasks,
            commands::cancel_background_task,
            commands::list_agents,
            commands::list_agent_definitions,
            commands::create_agent_definition,
            commands::delete_agent_definition,
            // Inter-agent message history (Phase D C3)
            commands::list_agent_messages,
            commands::list_agent_message_teams,
            commands::record_agent_message,
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
        ])
        .setup(|app| {
            let state = commands::AppState::new();
            app.manage(state);

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

            // System tray configuration
            let show_item = MenuItemBuilder::with_id("show", "Show Shannon").build(app)?;
            let new_session_item =
                MenuItemBuilder::with_id("new-session", "New Session").build(app)?;
            let check_updates_item =
                MenuItemBuilder::with_id("check-updates", "Check for Updates").build(app)?;
            let status_item =
                MenuItemBuilder::with_id("status", "Status: anthropic / claude-sonnet-4-6")
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

            let _tray = TrayIconBuilder::new()
                .tooltip("Shannon AI Assistant — anthropic / claude-sonnet-4-6")
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
