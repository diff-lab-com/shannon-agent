//! Screenshot rendering for predefined UI scenes.
//!
//! Renders scenes using ratatui's `TestBackend` (no real terminal needed)
//! and writes the buffer content to text files for AI analysis.

use std::path::Path;

use ratatui::{
    backend::TestBackend,
    buffer::Buffer,
    Terminal,
};

use crate::repl::ReplState;
use crate::widgets::{ChatWidget, ChatRole, MainLayoutWidget, PromptWidget};
use crate::repl::render::{render_completion_suggestions, render_permission_dialog};

// ── Scene data ─────────────────────────────────────────────────────

struct SceneData {
    state: ReplState,
    chat: ChatWidget,
    prompt: PromptWidget,
    name: &'static str,
    filename: &'static str,
}

// ── Public entry point ─────────────────────────────────────────────

/// Render all predefined scenes to text files in the given directory.
pub fn render_all_scenes(output_dir: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    std::fs::create_dir_all(output_dir)?;

    let scenes = vec![
        scene_default(),
        scene_completion(),
        scene_model_picker(),
        scene_chat(),
        scene_permission(),
    ];

    for scene in &scenes {
        let text = render_scene(scene);
        let path = output_dir.join(scene.filename);
        std::fs::write(&path, &text)?;
        eprintln!("  Wrote {}", path.display());
    }

    eprintln!(
        "Screenshot complete: {} scene(s) written to {}",
        scenes.len(),
        output_dir.display()
    );
    Ok(())
}

// ── Rendering ──────────────────────────────────────────────────────

const WIDTH: u16 = 100;
const HEIGHT: u16 = 30;

fn render_scene(scene: &SceneData) -> String {
    let backend = TestBackend::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(backend).expect("TestBackend init failed");

    let state = scene.state.clone();
    let chat = &scene.chat;
    let prompt = &scene.prompt;
    let spinner = &state.spinner;

    terminal
        .draw(|f| {
            let pb = if state.progress_bar_visible {
                Some(&state.progress_bar)
            } else {
                None
            };

            // Base layout — always rendered
            MainLayoutWidget::render_complete_with_spinner(
                f,
                chat,
                prompt,
                &state.status,
                state.model.as_deref(),
                Some(state.tokens_used),
                &state.working_directory,
                Some(spinner),
                pb,
            );

            // Overlays (mutually exclusive in normal rendering order)

            // Permission dialog
            if let Some(ref dialog) = state.permission_dialog {
                render_permission_dialog(f, f.area(), dialog);
            }

            // Model picker
            if let Some(ref mp) = state.model_picker {
                mp.render(f, f.area());
            }

            // Completion suggestions popup
            if !state.completion_suggestions.is_empty() {
                render_completion_suggestions(
                    f,
                    f.area(),
                    &state.completion_suggestions,
                    state.completion_suggestion_index,
                );
            }
        })
        .expect("TestBackend draw failed");

    let buf = terminal.backend().buffer().clone();
    buffer_to_text(&buf, WIDTH, HEIGHT, scene.name)
}

// ── Buffer → text ──────────────────────────────────────────────────

fn buffer_to_text(buf: &Buffer, w: u16, h: u16, name: &str) -> String {
    let mut out = String::new();

    // Header comment for AI context
    out.push_str(&format!(
        "# Shannon Screenshot: {name}\n\
         # Terminal size: {w}x{h}\n\
         # Format: plain text from ratatui Buffer (styling lost, layout preserved)\n\n"
    ));

    for row in 0..h {
        let mut line = String::new();
        for col in 0..w {
            let cell = buf.cell((col, row)).expect("cell in bounds");
            line.push_str(cell.symbol());
        }
        let trimmed = line.trim_end();
        out.push_str(trimmed);
        out.push('\n');
    }

    out
}

// ── Scene builders ─────────────────────────────────────────────────

fn scene_default() -> SceneData {
    SceneData {
        state: ReplState::default(),
        chat: ChatWidget::new(100),
        prompt: PromptWidget::new(),
        name: "default",
        filename: "01_default.txt",
    }
}

fn scene_completion() -> SceneData {
    let mut state = ReplState::default();
    let mut prompt = PromptWidget::new();
    let mut chat = ChatWidget::new(100);

    // Simulate typing "/he"
    prompt.set_input("/he".to_string());

    state.completion_suggestions = vec![
        "/help".to_string(),
        "/help-models".to_string(),
    ];
    state.completion_suggestion_index = 0;

    // Add prior messages so the chat area isn't empty
    chat.add_message(ChatRole::User, "What is Shannon?".to_string());
    chat.add_message(
        ChatRole::Assistant,
        "Shannon is an AI-powered code assistant built in Rust.".to_string(),
    );

    SceneData {
        state,
        chat,
        prompt,
        name: "completion",
        filename: "02_completion.txt",
    }
}

fn scene_model_picker() -> SceneData {
    let mut state = ReplState::default();
    let chat = ChatWidget::new(100);
    let prompt = PromptWidget::new();

    state.model_picker = Some(
        crate::widgets::select::ModelPickerWidget::new(Some("claude-sonnet-4-20250514")),
    );

    SceneData {
        state,
        chat,
        prompt,
        name: "model_picker",
        filename: "03_model_picker.txt",
    }
}

fn scene_chat() -> SceneData {
    let mut state = ReplState::default();
    let mut chat = ChatWidget::new(100);
    let prompt = PromptWidget::new();

    state.status = "Ready".to_string();
    state.tokens_used = 4237;

    chat.add_message(ChatRole::User, "Explain the ownership system in Rust".to_string());
    chat.add_message(
        ChatRole::Assistant,
        "Rust ownership is based on three rules:\n\
         1. Each value has one owner\n\
         2. When the owner goes out of scope, the value is dropped\n\
         3. There can only be one mutable reference or many immutable references"
            .to_string(),
    );
    chat.add_message(ChatRole::User, "Show me an example".to_string());
    chat.add_message(
        ChatRole::Assistant,
        "Here is a simple example of ownership transfer.".to_string(),
    );
    chat.add_message(ChatRole::Tool, "bash: cargo check".to_string());
    chat.add_message(ChatRole::System, "Compilation successful".to_string());

    SceneData {
        state,
        chat,
        prompt,
        name: "chat",
        filename: "04_chat.txt",
    }
}

fn scene_permission() -> SceneData {
    let mut state = ReplState::default();
    let mut chat = ChatWidget::new(100);
    let prompt = PromptWidget::new();

    chat.add_message(ChatRole::User, "Add error handling to src/main.rs".to_string());
    chat.add_message(
        ChatRole::Assistant,
        "I will modify src/main.rs to add proper error handling.".to_string(),
    );

    state.permission_dialog = Some(shannon_core::permissions::PermissionPrompt {
        id: uuid::Uuid::new_v4(),
        tool_name: "write_file".to_string(),
        tool_input: serde_json::json!({
            "path": "src/main.rs",
            "content": "fn main() -> anyhow::Result<()> { ... }"
        }),
        risk_level: shannon_core::permissions::RiskLevel::Low,
        description: "Write to src/main.rs".to_string(),
        is_confirmation: false,
        diff_preview: Some(
            "--- a/src/main.rs\n\
             +++ b/src/main.rs\n\
             @@ -1,5 +1,8 @@\n\
             fn main() {\n\
             -    println!(\"hello\");\n\
             +    match run() {\n\
             +        Ok(()) => (),\n\
             +        Err(e) => eprintln!(\"Error: {e}\"),\n\
             +    }\n\
             }"
                .to_string(),
        ),
        is_destructive: false,
    });

    SceneData {
        state,
        chat,
        prompt,
        name: "permission",
        filename: "05_permission.txt",
    }
}
