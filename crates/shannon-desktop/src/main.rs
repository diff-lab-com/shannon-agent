//! Shannon Code desktop application entry point.
//!
//! Uses Tauri v2 to wrap the Shannon AI assistant in a native desktop window
//! with a web-based chat UI. The Rust backend handles LLM communication,
//! tool execution, and state management via Tauri IPC commands.

#[cfg(feature = "tauri")]
fn main() {
    use shannon_desktop::commands;
    use tauri::Manager;

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::send_message,
            commands::get_conversation,
            commands::list_models,
            commands::get_status,
            commands::cancel_query,
            commands::list_tools,
            commands::configure,
        ])
        .setup(|app| {
            // Initialize the query engine state
            let state = commands::AppState::new();
            app.manage(state);
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
