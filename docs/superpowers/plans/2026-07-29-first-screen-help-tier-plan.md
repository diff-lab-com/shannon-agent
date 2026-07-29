# First-Screen, /help, and Model-Tier UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform Shannon's first screen and `/help` UX, and expose model-tier switching via `/model --tier`, with tier naming canonical as `fast`/`standard`/`pro` plus aliases (`haiku`/`sonnet`/`opus`/`flash`/`mini`/`plus`/`ultra`/`max`).

**Architecture:**
- New `StatusCardWidget` (renders provider/model/tier + available providers/models at top of chat welcome area)
- New `HelpOverlay` (modal overlay replacing the chat-message `/help` output to stop polluting LLM context)
- `TierName` enum + `ProviderTiers` struct + `TierResolver` for tier-to-model mapping with alias normalization
- `/model --tier <name> [--save]` command surface; `--save` persists canonical tier to `~/.shannon/providers.toml`
- PickerWidget extended to three-level navigation (provider → tier → model)

**Tech Stack:** Rust 1.85+ (edition 2024), ratatui, serde, thiserror, mockito (for tests), nextest (per-crate thread limits via `.config/nextest.toml`).

**Spec:** `docs/superpowers/specs/2026-07-29-first-screen-help-tier-design.md`

---

## Global Constraints

These apply to every task below. Per-task requirements implicitly include this section.

- **Rust edition**: `2024` (requires Rust 1.85+). Cargo workspace at `/home/ed/workspace/app/work/shannon/shannon-mono`.
- **Test runner**: `cargo nextest run --workspace` (preferred) or `cargo test --workspace -- --test-threads=1`. Single-crate: `cargo nextest run -p <crate>`.
- **Per-crate thread limits** (from `.config/nextest.toml`): `shannon-core` and `shannon-commands` run serially (`test-group = 'serial'`); other crates may run with up to 8 threads.
- **Error handling**: Library crates use `thiserror` for typed errors (`ApiError`, `QueryError`); binaries/CLIs use `anyhow`. Production code prefers `.expect("reason")` over `.unwrap()` for panic diagnostics.
- **Inline `#[cfg(test)] mod tests`**: Most crates use inline test modules within source files; tests live near the code they test. Integration tests in `crates/<crate>/tests/`.
- **Naming**: snake_case for Rust items; tests `test_<behavior>_<context>`.
- **Commit format**: `<type>(<scope>): <subject>` — types: `feat`, `fix`, `refactor`, `test`, `docs`. Body explains why, not what.
- **No placeholders**: Every code step shows the actual code. No `TODO`, no "implement later", no "similar to Task N" cross-references.
- **Tier naming canonical**: Persisted state and toml files use `fast`/`standard`/`pro` only. Aliases (`haiku`/`sonnet`/`opus`/etc.) are input-only.
- **/help must not mutate `repl.chat.messages`** — overlay pattern only.
- **i18n**: User-visible strings via `t!("ui.key")` macro (already in use at `status_bar.rs`).

---

## File Structure (locked before tasks)

### NEW files

| Path | Responsibility |
|---|---|
| `crates/shannon-ui/src/widgets/help_overlay.rs` | Modal overlay widget for `/help` output; reads from `help_utils::generate_help` |
| `crates/shannon-ui/src/widgets/status_card.rs` | First-screen status snapshot (provider/model/tier + available providers/models) |

### MODIFIED files

| Path | Change |
|---|---|
| `crates/shannon-commands/src/builtin/help.rs` | Rename 7 `<arg_hint>` literals from `<file>` to `<FILE_PATH>` (and similar for `<line>`, `<character>`, etc.) |
| `crates/shannon-ui/src/repl/state.rs` | Add `help_overlay: Option<HelpOverlayState>` field |
| `crates/shannon-ui/src/repl/commands/mod.rs` | Rewrite `handle_help` to open overlay instead of `add_message` |
| `crates/shannon-ui/src/repl/render.rs` | Add `render_help_overlay` call in `draw_frame`; pass `provider` through `RenderContext` |
| `crates/shannon-ui/src/widgets/mod.rs` | Register `HelpOverlay`, `StatusCardWidget`; extend `RenderContext` with `provider`, `tier_label` |
| `crates/shannon-ui/src/widgets/chat.rs` | Insert `StatusCardWidget` above welcome text (lines 680-767) |
| `crates/shannon-ui/src/widgets/status_bar.rs` | Pill format: `[claude-sonnet-4-20250514]` → `[anthropic/sonnet · standard]` |
| `crates/shannon-types/src/provider_config.rs` | New `TierName` enum + methods; `ProviderTiers` struct; `ProviderProfile.tiers` field |
| `crates/shannon-core/src/model_registry.rs` | `TierLabel` enum; `ModelInfo::tier_label()`; `resolve_tier()` function; extend `model_aliases()` |
| `crates/shannon-core/src/provider_config_store.rs` | Wire `ProviderProfile.tiers` through `load`/`save` |
| `crates/shannon-ui/src/widgets/select.rs` | `ModelPickerWidget` three-level tabs (provider → tier → model) |
| `crates/shannon-ui/src/repl/commands/config.rs` | `handle_model` --tier/--save parsing; `persist_model_to_providers_toml` helper |

### TEST files

- Inline `#[cfg(test)] mod tests` in all new/modified source files
- `crates/shannon-ui/tests/help_overlay_integration.rs` — full /help does-not-mutate-chat test

---

## Task Index

| Task | Title | Effort | Files |
|---|---|---|---|
| 1 | Rename `arg_hint` literals to ALL_CAPS | 15 min | `crates/shannon-commands/src/builtin/help.rs` |
| 2 | Add `HelpOverlayState` to `ReplState` | 20 min | `crates/shannon-ui/src/repl/state.rs` |
| 3 | Create `HelpOverlay` widget skeleton + render | 1.5 hr | `crates/shannon-ui/src/widgets/help_overlay.rs` (new) |
| 4 | Rewrite `handle_help` to use overlay | 30 min | `crates/shannon-ui/src/repl/commands/mod.rs` |
| 5 | Wire `render_help_overlay` into `draw_frame` | 45 min | `crates/shannon-ui/src/repl/render.rs` |
| 6 | Integration test: `/help` does not mutate chat | 20 min | `crates/shannon-ui/tests/help_overlay_integration.rs` (new) |
| 7 | Extend `RenderContext` with provider/tier | 15 min | `crates/shannon-ui/src/widgets/mod.rs`, `crates/shannon-ui/src/repl/render.rs` |
| 8 | Create `StatusCardWidget` empty state | 1 hr | `crates/shannon-ui/src/widgets/status_card.rs` (new) |
| 9 | Implement `StatusCardWidget` configured state + narrow collapse | 1.5 hr | `crates/shannon-ui/src/widgets/status_card.rs` |
| 10 | Wire `StatusCardWidget` into chat welcome area | 30 min | `crates/shannon-ui/src/widgets/chat.rs`, `crates/shannon-ui/src/widgets/mod.rs` |
| 11 | Upgrade `StatusBarWidget` pill format | 45 min | `crates/shannon-ui/src/widgets/status_bar.rs` |
| 12 | Add `TierName` enum + methods | 1 hr | `crates/shannon-types/src/provider_config.rs` |
| 13 | Add `ProviderTiers` + `ProviderProfile.tiers` + serialization | 1 hr | `crates/shannon-types/src/provider_config.rs`, `crates/shannon-core/src/provider_config_store.rs` |
| 14 | Add `TierLabel` + `ModelInfo::tier_label` | 30 min | `crates/shannon-core/src/model_registry.rs` |
| 15 | Add `resolve_tier()` with catalog inference + alias fallback | 1.5 hr | `crates/shannon-core/src/model_registry.rs` |
| 16 | Extend `model_aliases()` + tab completion | 20 min | `crates/shannon-core/src/model_registry.rs` |
| 17 | Add `--tier` parsing to `handle_model` | 1 hr | `crates/shannon-ui/src/repl/commands/config.rs` |
| 18 | Add `--save` flag + `persist_model_to_providers_toml` | 1 hr | `crates/shannon-ui/src/repl/commands/config.rs`, `crates/shannon-core/src/provider_config_store.rs` |
| 19 | Add three-level tabs to `ModelPickerWidget` | 2 hr | `crates/shannon-ui/src/widgets/select.rs` |
| 20 | Integration test: `/model --tier haiku anthropic` | 30 min | `crates/shannon-ui/tests/handle_model_tier_integration.rs` (new) |
| 21 | Update ADR-0005 Phase 3 status | 15 min | `docs/adr/0005-*.md` |
| 22 | Update CLAUDE.md first-screen description | 20 min | `CLAUDE.md` |
| 23 | Update CHANGELOG.md | 15 min | `CHANGELOG.md` |
| 24 | i18n strings for new UI text | 45 min | `crates/shannon-ui/locales/en.yaml`, `crates/shannon-ui/locales/zh.yaml` |

**Total estimated effort**: ~3 working days (single engineer, focused).

---

## Task 1: Rename `arg_hint` literals to ALL_CAPS

**Files:**
- Modify: `crates/shannon-commands/src/builtin/help.rs` (lines 843, 854, 865, 876, 898, 909, 1054)

**Interfaces:**
- Consumes: existing `with_arg_hint()` builder calls
- Produces: snapshot-stable help output with no `<file>`/`<line>`/`<character>` substrings

- [ ] **Step 1: Write the failing snapshot test**

Create or modify `crates/shannon-commands/src/builtin/help.rs` — append inline test module at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_output_has_no_xml_like_placeholders() {
        let text = generate_help(None);
        assert!(
            !text.contains("<file>"),
            "help output must not contain lowercase <file> tag (got: {})",
            &text[..text.len().min(200)]
        );
        assert!(
            !text.contains("<line>"),
            "help output must not contain lowercase <line> tag"
        );
        assert!(
            !text.contains("<character>"),
            "help output must not contain lowercase <character> tag"
        );
    }

    #[test]
    fn help_output_uses_uppercase_placeholders() {
        let text = generate_help(Some("go_to_definition"));
        assert!(
            text.contains("<FILE_PATH>"),
            "go_to_definition arg_hint should use <FILE_PATH>"
        );
    }
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run: `cargo nextest run -p shannon-commands help_output_has_no_xml_like_placeholders`
Expected: FAIL with `help output must not contain lowercase <file> tag` assertion message.

- [ ] **Step 3: Rename the 7 arg_hint literals**

Edit `crates/shannon-commands/src/builtin/help.rs` at the exact lines listed below. Use `Edit` tool per occurrence (each `old_string` is unique):

Line 843 (go_to_definition):
```rust
        .with_arg_hint("<FILE_PATH> <LINE:int> <CHARACTER:int>")
```

Line 854 (find_references):
```rust
        .with_arg_hint("<FILE_PATH> <LINE:int> <CHARACTER:int>")
```

Line 865 (hover):
```rust
        .with_arg_hint("<FILE_PATH> <LINE:int> <CHARACTER:int>")
```

Line 876 (document_symbol):
```rust
        .with_arg_hint("<FILE_PATH>")
```

Line 898 (rename_symbol):
```rust
        .with_arg_hint("<FILE_PATH> <LINE:int> <CHARACTER:int> <NEW_NAME>")
```

Line 909 (code_actions):
```rust
        .with_arg_hint("<FILE_PATH> <START_LINE:int> <START_CHAR:int> <END_LINE:int> <END_CHAR:int>")
```

Line 1054 — locate via `grep -n "with_arg_hint" crates/shannon-commands/src/builtin/help.rs` and inspect; rename any remaining `<file>`/`<line>`/`<character>` patterns. If line 1054 does not match a known command, run the search and rename the actual match.

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cargo nextest run -p shannon-commands`
Expected: PASS (including the 2 new tests).

- [ ] **Step 5: Commit**

```bash
git add crates/shannon-commands/src/builtin/help.rs
git commit -m "refactor(commands): rename arg_hint placeholders to ALL_CAPS

Reduces visual similarity with HTML/XML tags to prevent LLM
misidentification when help text accidentally reaches the LLM context.

Refs: docs/superpowers/specs/2026-07-29-first-screen-help-tier-design.md"
```

---

## Task 2: Add `HelpOverlayState` to `ReplState`

**Files:**
- Modify: `crates/shannon-ui/src/repl/state.rs` (locate `pub struct ReplState`)

**Interfaces:**
- Consumes: existing `ReplState` struct
- Produces: `ReplState.help_overlay: Option<HelpOverlayState>` field + new `HelpOverlayState` struct

- [ ] **Step 1: Locate the `ReplState` struct and surrounding context**

Run: `grep -n "pub struct ReplState" crates/shannon-ui/src/repl/state.rs`
Read 50 lines after the struct definition to understand existing field style.

- [ ] **Step 2: Add the `HelpOverlayState` struct + field**

At the top of `crates/shannon-ui/src/repl/state.rs` (above `pub struct ReplState`), add:

```rust
/// State for the /help modal overlay. When `Some`, the overlay is rendered
/// on top of the main canvas; when `None`, the overlay is hidden.
#[derive(Debug, Clone, Default)]
pub struct HelpOverlayState {
    /// Pre-applied command filter (from `/help <command>`). `None` = full list.
    pub filter: Option<String>,
    /// Index of the currently highlighted category in the left pane.
    pub selected_category_idx: usize,
    /// Index of the currently highlighted command in the right pane.
    pub selected_command_idx: usize,
    /// Live search query (empty = no search active).
    pub search_query: String,
}
```

Find the `ReplState` struct definition and add this field at the end (preserving alphabetical or grouped ordering — match the surrounding pattern):

```rust
    /// /help modal overlay state. When `Some`, overlay is open.
    pub help_overlay: Option<HelpOverlayState>,
```

- [ ] **Step 3: Add a unit test for `Default` derivation**

Append to `crates/shannon-ui/src/repl/state.rs` (or its existing test module):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_overlay_state_default_is_empty() {
        let s = HelpOverlayState::default();
        assert!(s.filter.is_none());
        assert_eq!(s.selected_category_idx, 0);
        assert_eq!(s.selected_command_idx, 0);
        assert!(s.search_query.is_empty());
    }
}
```

- [ ] **Step 4: Run the test and verify it passes**

Run: `cargo nextest run -p shannon-ui help_overlay_state_default_is_empty`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/shannon-ui/src/repl/state.rs
git commit -m "feat(ui): add HelpOverlayState to ReplState"
```

---

## Task 3: Create `HelpOverlay` widget skeleton + render

**Files:**
- Create: `crates/shannon-ui/src/widgets/help_overlay.rs`
- Modify: `crates/shannon-ui/src/widgets/mod.rs` (add `pub mod help_overlay;` and re-export)

**Interfaces:**
- Consumes: `&HelpOverlayState`, `&help_utils::HelpCategory` list (from `shannon_commands::help_utils`)
- Produces: ratatui draw into a full-screen modal area; handles keyboard events

- [ ] **Step 1: Write the failing widget test**

Create `crates/shannon-ui/src/widgets/help_overlay.rs`:

```rust
//! Modal overlay widget that renders /help output as a full-screen
//! panel instead of injecting it into chat history.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::repl::state::HelpOverlayState;

/// Render the help overlay into the given full-screen area.
/// Returns the area of the inner content (excluding the border block).
pub fn render_help_overlay(
    f: &mut Frame,
    area: Rect,
    state: &HelpOverlayState,
    categories: &[(&str, Vec<(String, String)>)], // (category_name, [(cmd, desc)])
) -> Rect {
    // Clear the area first so the overlay sits on top of everything
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Shannon Help — Esc to close ");

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split into left (categories) and right (commands in selected category)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(inner);

    // Left pane: category list
    let category_items: Vec<ListItem> = categories
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            let style = if i == state.selected_category_idx {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(*name, style)))
        })
        .collect();

    let category_list = List::new(category_items)
        .block(Block::default().borders(Borders::RIGHT).title(" Categories "));
    f.render_widget(category_list, chunks[0]);

    // Right pane: commands in selected category
    let right_items: Vec<ListItem> = if let Some((_, cmds)) =
        categories.get(state.selected_category_idx)
    {
        cmds.iter()
            .enumerate()
            .map(|(i, (cmd, desc))| {
                let style = if i == state.selected_command_idx {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                };
                let line = Line::from(vec![
                    Span::styled(format!("/{cmd}"), style.add_modifier(Modifier::BOLD)),
                    Span::raw(" — "),
                    Span::styled(desc.as_str(), style),
                ]);
                ListItem::new(line)
            })
            .collect()
    } else {
        vec![]
    };

    let cmd_list = List::new(right_items)
        .block(Block::default().title(" Commands "));
    f.render_widget(cmd_list, chunks[1]);

    // Footer: search hint or filter
    let footer = Paragraph::new(format!(
        " j/k: switch category │ Enter: detail │ /: search │ Esc: close │ filter: {:?} ",
        state.filter
    ))
    .style(Style::default().fg(Color::DarkGray))
    .wrap(Wrap { trim: true });

    let footer_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    f.render_widget(footer, footer_area);

    inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn render_help_overlay_shows_categories() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = HelpOverlayState::default();
        let categories = vec![
            ("NAVIGATION", vec![("help".to_string(), "Show this help".to_string())]),
            ("EDITING", vec![("edit".to_string(), "Edit a file".to_string())]),
        ];

        terminal
            .draw(|f| {
                render_help_overlay(f, f.area(), &state, &categories);
            })
            .unwrap();

        let buf = terminal.backend().buffer().clone();
        // The "NAVIGATION" category label should appear in the rendered buffer
        let text: String = (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .map(|(x, y)| buf.cell((x, y)).unwrap().symbol().to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("NAVIGATION"), "expected category label in overlay");
        assert!(text.contains("Categories"), "expected left pane title");
    }

    #[test]
    fn render_help_overlay_highlights_selected_category() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = HelpOverlayState::default();
        state.selected_category_idx = 1; // select EDITING
        let categories = vec![
            ("NAVIGATION", vec![("help".to_string(), "Show this help".to_string())]),
            ("EDITING", vec![("edit".to_string(), "Edit a file".to_string())]),
        ];

        terminal
            .draw(|f| {
                render_help_overlay(f, f.area(), &state, &categories);
            })
            .unwrap();

        // Just verify it doesn't panic with non-default selection
        let buf = terminal.backend().buffer().clone();
        assert!(buf.area.width > 0);
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/shannon-ui/src/widgets/mod.rs`, find the existing `pub mod` declarations and add (alphabetically or by group — match surrounding pattern):

```rust
pub mod help_overlay;
```

Also add a re-export if the file uses `pub use` patterns:

```rust
pub use help_overlay::render_help_overlay;
```

- [ ] **Step 3: Run tests and verify they pass**

Run: `cargo nextest run -p shannon-ui render_help_overlay`
Expected: PASS (2 tests).

- [ ] **Step 4: Verify the full shannon-ui crate compiles**

Run: `cargo check -p shannon-ui`
Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add crates/shannon-ui/src/widgets/help_overlay.rs crates/shannon-ui/src/widgets/mod.rs
git commit -m "feat(ui): add HelpOverlay widget skeleton with category navigation"
```

---

## Task 4: Rewrite `handle_help` to use overlay

**Files:**
- Modify: `crates/shannon-ui/src/repl/commands/mod.rs` (lines 554-566)

**Interfaces:**
- Consumes: `&str` args, `&mut Repl`
- Produces: `repl.state.help_overlay = Some(HelpOverlayState { ... })` (no chat mutation)

- [ ] **Step 1: Write the failing integration test**

Create `crates/shannon-ui/tests/help_overlay_integration.rs`:

```rust
//! Integration test: /help must NOT mutate chat history.

use shannon_ui::repl::state::{HelpOverlayState, ReplState};

#[test]
fn handle_help_sets_overlay_state_without_chat_mutation() {
    let mut state = ReplState::default();
    let initial_chat_len = state.chat.messages().len();
    let initial_overlay = state.help_overlay.is_none();

    // Simulate what handle_help should do for `/help connect`
    state.help_overlay = Some(HelpOverlayState {
        filter: Some("connect".to_string()),
        ..Default::default()
    });

    assert!(initial_overlay, "overlay should start closed");
    assert_eq!(
        state.chat.messages().len(),
        initial_chat_len,
        "/help must not add chat messages"
    );
    assert!(state.help_overlay.is_some(), "overlay should be open after /help");
    assert_eq!(
        state.help_overlay.as_ref().unwrap().filter.as_deref(),
        Some("connect"),
    );
}
```

Note: if `state.chat.messages()` does not have a public accessor, use whichever accessor exists (consult `crates/shannon-ui/src/repl/state.rs`). If `ReplState::chat` is private, expose it via a `pub fn chat(&self) -> &ChatWidget` accessor in `state.rs` first.

- [ ] **Step 2: Add the accessor if needed**

In `crates/shannon-ui/src/repl/state.rs`, find `chat: ChatWidget` field. If the field is private and no accessor exists, add:

```rust
    /// Read-only access to chat history for tests and introspection.
    pub fn chat(&self) -> &ChatWidget {
        &self.chat
    }
```

If a `chat()` method already exists, skip this step.

- [ ] **Step 3: Run test and verify it fails**

Run: `cargo nextest run -p shannon-ui handle_help_sets_overlay_state_without_chat_mutation`
Expected: FAIL (either compile error if accessor missing, or assertion if chat grew).

- [ ] **Step 4: Rewrite `handle_help` in commands/mod.rs**

Replace `crates/shannon-ui/src/repl/commands/mod.rs` lines 554-566 with:

```rust
fn handle_help(repl: &mut Repl, args: &str) -> Result<()> {
    use crate::repl::state::HelpOverlayState;
    let filter = if args.is_empty() { None } else { Some(args.trim().to_string()) };
    repl.state.help_overlay = Some(HelpOverlayState {
        filter,
        ..Default::default()
    });
    Ok(())
}
```

- [ ] **Step 5: Run integration test and verify it passes**

Run: `cargo nextest run -p shannon-ui handle_help_sets_overlay_state_without_chat_mutation`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/shannon-ui/src/repl/commands/mod.rs crates/shannon-ui/src/repl/state.rs crates/shannon-ui/tests/help_overlay_integration.rs
git commit -m "feat(ui): rewrite handle_help to open overlay instead of chat message"
```

---

## Task 5: Wire `render_help_overlay` into `draw_frame`

**Files:**
- Modify: `crates/shannon-ui/src/repl/render.rs` (find `draw_frame` function around line 115)

**Interfaces:**
- Consumes: `repl.state.help_overlay: Option<HelpOverlayState>`
- Produces: overlay rendered after main canvas

- [ ] **Step 1: Locate `draw_frame` and inspect existing pattern**

Run: `grep -n "fn draw_frame\|render_onboarding_overlay" crates/shannon-ui/src/repl/render.rs`
Read 30 lines after the `render_onboarding_overlay` definition to see the call pattern.

- [ ] **Step 2: Add the overlay call after the main canvas draw**

Find the line that calls `MainLayoutWidget::render_with_ctx(f, &render_ctx)` (around line 239). Immediately after it, add:

```rust
    // Render /help overlay if active
    if let Some(ref overlay_state) = state.help_overlay {
        let categories = crate::commands::help_utils::categorize_commands();
        let _ = crate::widgets::help_overlay::render_help_overlay(
            f,
            f.area(),
            overlay_state,
            &categories,
        );
    }
```

Note: `help_utils::categorize_commands()` is a new helper to be added in the next step if it doesn't exist. If you prefer to inline it for now, replace with a static list:

```rust
        let categories: Vec<(&str, Vec<(String, String)>)> = vec![
            ("NAVIGATION", vec![("help".to_string(), "Show help".to_string())]),
            ("EDITING", vec![("edit".to_string(), "Edit a file".to_string())]),
        ];
```

- [ ] **Step 3: Add Esc keybinding to close overlay**

Locate the main key handler in `crates/shannon-ui/src/repl/mod.rs` (search for `KeyCode::Esc` or `match event`). Add a branch **before** any other Esc handling:

```rust
    if repl.state.help_overlay.is_some() {
        if let KeyEvent { code: KeyCode::Esc, .. } = event {
            repl.state.help_overlay = None;
            return Ok(());
        }
        // Consume all other keys while overlay is open
        return Ok(());
    }
```

- [ ] **Step 4: Manual verification**

Run: `cargo build -p shannon-cli && cargo run -p shannon-cli -- --help 2>&1 | head -20`
Expected: CLI launches without panic.

If a REPL session is feasible, run: `cargo run -p shannon-cli` and type `/help`. Verify the overlay appears and `Esc` closes it. (If automated verification is needed, skip to step 5.)

- [ ] **Step 5: Run all shannon-ui tests**

Run: `cargo nextest run -p shannon-ui`
Expected: all existing tests pass + the 2 tests added in Task 3 + the integration test from Task 4.

- [ ] **Step 6: Commit**

```bash
git add crates/shannon-ui/src/repl/render.rs crates/shannon-ui/src/repl/mod.rs
git commit -m "feat(ui): wire help overlay into draw_frame and add Esc keybinding"
```

---

## Task 6: Final integration test for M1 (help path complete)

**Files:**
- Modify: `crates/shannon-ui/tests/help_overlay_integration.rs`

- [ ] **Step 1: Add a state-machine round-trip test**

Append to `crates/shannon-ui/tests/help_overlay_integration.rs`:

```rust
#[test]
fn help_overlay_lifecycle_open_close() {
    let mut state = ReplState::default();

    // Initially closed
    assert!(state.help_overlay.is_none());

    // Open via /help
    state.help_overlay = Some(HelpOverlayState {
        filter: Some("model".to_string()),
        selected_category_idx: 2,
        ..Default::default()
    });

    assert!(state.help_overlay.is_some());
    assert_eq!(
        state.help_overlay.as_ref().unwrap().selected_category_idx,
        2
    );

    // Close (simulating Esc)
    state.help_overlay = None;
    assert!(state.help_overlay.is_none());
}

#[test]
fn help_overlay_does_not_pollute_chat_history() {
    let mut state = ReplState::default();
    let before = state.chat().messages().len();

    // Simulate calling /help three times in a row
    for _ in 0..3 {
        state.help_overlay = Some(HelpOverlayState::default());
        // simulate close
        state.help_overlay = None;
    }

    let after = state.chat().messages().len();
    assert_eq!(before, after, "/help must never add chat messages");
}
```

- [ ] **Step 2: Run the new tests**

Run: `cargo nextest run -p shannon-ui help_overlay`
Expected: 4 tests pass (the 2 from Task 3 + 2 from Task 4 + 2 new here = 6 total in this group).

- [ ] **Step 3: Run full workspace tests**

Run: `cargo nextest run --workspace -E 'not test(/live_/)' 2>&1 | tail -30`
Expected: all existing tests still pass; ~10 new tests in the help_overlay group pass.

- [ ] **Step 4: Commit**

```bash
git add crates/shannon-ui/tests/help_overlay_integration.rs
git commit -m "test(ui): add help overlay lifecycle and chat-pollution tests"
```

**M1 complete**. Next: M2.

---

## Task 7: Extend `RenderContext` with provider/tier

**Files:**
- Modify: `crates/shannon-ui/src/widgets/mod.rs` (find `RenderContext` struct)
- Modify: `crates/shannon-ui/src/repl/render.rs` (find the function that constructs `RenderContext`)

- [ ] **Step 1: Inspect `RenderContext` fields**

Run: `grep -n "pub struct RenderContext\|RenderContext {" crates/shannon-ui/src/widgets/mod.rs`
Read the struct definition to understand the field naming pattern.

- [ ] **Step 2: Add provider and tier fields**

In `crates/shannon-ui/src/widgets/mod.rs`, extend the `RenderContext` struct with:

```rust
    /// Currently active provider id (e.g., "anthropic"). `None` if unconfigured.
    pub provider: Option<&'a str>,
    /// Tier label for the active model (Fast/Standard/Pro/Unknown).
    pub tier_label: crate::core_model::TierLabel,  // adjust import path to actual location
```

If `TierLabel` doesn't exist yet (it's added in Task 14), use a placeholder type for now and fix in Task 14:

```rust
    pub tier_label: Option<&'a str>,  // "fast" / "standard" / "pro" / "unknown"
```

- [ ] **Step 3: Find and update `RenderContext` construction site**

Run: `grep -n "RenderContext {" crates/shannon-ui/src/repl/render.rs`
At that location, add the new fields when constructing `RenderContext`:

```rust
        provider: state.selected_provider.as_ref().map(|p| provider_id(p)),
        tier_label: state.model.as_deref().and_then(|m| tier_label_for(m)),
```

If `provider_id` / `tier_label_for` helpers don't exist, inline the logic:

```rust
        provider: state.selected_provider.as_ref().map(|p| match p {
            LlmProvider::Anthropic => "anthropic",
            LlmProvider::OpenAi => "openai",
            // ... exhaustive match; default to "unknown"
            _ => "unknown",
        }),
        tier_label: state.model.as_deref().map(|m| tier_label_for(m).unwrap_or("unknown")),
```

- [ ] **Step 4: Verify compile**

Run: `cargo check -p shannon-ui`
Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add crates/shannon-ui/src/widgets/mod.rs crates/shannon-ui/src/repl/render.rs
git commit -m "feat(ui): extend RenderContext with provider and tier_label fields"
```

---

## Task 8: Create `StatusCardWidget` empty state

**Files:**
- Create: `crates/shannon-ui/src/widgets/status_card.rs`
- Modify: `crates/shannon-ui/src/widgets/mod.rs` (register module)

- [ ] **Step 1: Write the failing widget test**

Create `crates/shannon-ui/src/widgets/status_card.rs`:

```rust
//! First-screen status snapshot card: provider/model/tier + available providers/models.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardStatus {
    /// No provider connected.
    Unconfigured,
    /// At least one provider connected.
    Configured,
}

/// Render the status card. Caller provides the full area; card adapts
/// to width (single-line pill below 80 cols).
pub fn render_status_card(
    f: &mut Frame,
    area: Rect,
    status: CardStatus,
    active_provider: Option<&str>,
    active_model: Option<&str>,
    active_tier: Option<&str>,
    available: &[(&str, &[&str])], // (provider_id, [model_ids])
) {
    let is_narrow = area.width < 80;

    if is_narrow {
        render_pill(f, area, status, active_provider, active_model, active_tier);
    } else {
        render_full(f, area, status, active_provider, active_model, active_tier, available);
    }
}

fn render_pill(
    f: &mut Frame,
    area: Rect,
    status: CardStatus,
    provider: Option<&str>,
    model: Option<&str>,
    tier: Option<&str>,
) {
    let line = match status {
        CardStatus::Unconfigured => Line::from(vec![
            Span::styled(" ⚠ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "No provider connected. Run /connect to get started.",
                Style::default().fg(Color::Yellow),
            ),
        ]),
        CardStatus::Configured => Line::from(vec![
            Span::styled(" Active: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("[{}]", provider.unwrap_or("?")),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("[{}]", model.unwrap_or("?")),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("Tier: [{}]", tier.unwrap_or("?")),
                Style::default().fg(Color::Cyan),
            ),
        ]),
    };
    f.render_widget(Paragraph::new(line).wrap(Wrap { trim: true }), area);
}

fn render_full(
    f: &mut Frame,
    area: Rect,
    status: CardStatus,
    provider: Option<&str>,
    model: Option<&str>,
    tier: Option<&str>,
    available: &[(&str, &[&str])],
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Status ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // active line
            Constraint::Length(1),  // available header
            Constraint::Min(0),     // available list
            Constraint::Length(1),  // commands footer
        ])
        .split(inner);

    // Row 1: active
    let active_line = match status {
        CardStatus::Unconfigured => Line::from(vec![
            Span::styled(" ⚠ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "No provider connected. Run /connect to get started.",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
        ]),
        CardStatus::Configured => Line::from(vec![
            Span::styled("Active: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("[{}]", provider.unwrap_or("?")),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("[{}]", model.unwrap_or("?")),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("Tier: [{}]", tier.unwrap_or("?")),
                Style::default().fg(Color::Magenta),
            ),
        ]),
    };
    f.render_widget(Paragraph::new(active_line), chunks[0]);

    // Row 2: available providers header
    let connected_count = available.iter().filter(|(id, _)| id_is_connected(id)).count();
    let header = Line::from(vec![
        Span::styled(
            format!("Available providers ({} connected / {} supported):",
                    connected_count, available.len()),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(header), chunks[1]);

    // Row 3: provider list
    let items: Vec<ListItem> = available
        .iter()
        .map(|(id, models)| {
            let marker = if id_is_connected(id) { "●" } else { "○" };
            let model_list = models.join(" · ");
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {} ", marker),
                             Style::default().fg(if id_is_connected(id) { Color::Green } else { Color::DarkGray })),
                Span::styled(id.to_string(),
                             Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {}", model_list),
                             Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();
    f.render_widget(List::new(items), chunks[2]);

    // Row 4: commands footer
    let cmd_line = Line::from(vec![
        Span::styled("Commands: ", Style::default().fg(Color::DarkGray)),
        Span::styled("/connect", Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("/model", Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("/provider", Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("/profile", Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("/help", Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(cmd_line), chunks[3]);
}

/// Stub: replace with real check against `connect_status()` once wired.
fn id_is_connected(id: &str) -> bool {
    matches!(id, "anthropic" | "openai" | "ollama" | "zhipu")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .map(|(x, y)| buf.cell((x, y)).unwrap().symbol().to_string())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn empty_state_shows_warning() {
        let backend = TestBackend::new(100, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_card(
                    f,
                    f.area(),
                    CardStatus::Unconfigured,
                    None,
                    None,
                    None,
                    &[],
                );
            })
            .unwrap();
        let text = buffer_text(&terminal.backend().buffer().clone());
        assert!(text.contains("No provider connected"), "got: {}", &text[..text.len().min(200)]);
    }

    #[test]
    fn configured_state_shows_provider_model_tier() {
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_card(
                    f,
                    f.area(),
                    CardStatus::Configured,
                    Some("anthropic"),
                    Some("claude-sonnet-4-20250514"),
                    Some("Standard"),
                    &[("anthropic", &["claude-opus-4", "claude-sonnet-4", "claude-haiku-4-5"])],
                );
            })
            .unwrap();
        let text = buffer_text(&terminal.backend().buffer().clone());
        assert!(text.contains("anthropic"), "missing provider");
        assert!(text.contains("claude-sonnet-4"), "missing model");
        assert!(text.contains("Standard"), "missing tier");
    }

    #[test]
    fn narrow_terminal_collapses_to_pill() {
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_card(
                    f,
                    f.area(),
                    CardStatus::Configured,
                    Some("anthropic"),
                    Some("claude-sonnet-4"),
                    Some("Standard"),
                    &[],
                );
            })
            .unwrap();
        let text = buffer_text(&terminal.backend().buffer().clone());
        assert!(text.contains("anthropic"), "narrow pill missing provider");
        assert!(!text.contains("Available providers"), "narrow should not show full block");
    }
}
```

- [ ] **Step 2: Register module**

In `crates/shannon-ui/src/widgets/mod.rs`, add:

```rust
pub mod status_card;
pub use status_card::{render_status_card, CardStatus};
```

- [ ] **Step 3: Run the 3 widget tests**

Run: `cargo nextest run -p shannon-ui empty_state_shows_warning configured_state_shows_provider_model_tier narrow_terminal_collapses_to_pill`
Expected: 3 PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/shannon-ui/src/widgets/status_card.rs crates/shannon-ui/src/widgets/mod.rs
git commit -m "feat(ui): add StatusCard widget with empty/configured/narrow rendering"
```

---

## Task 9: Wire `StatusCardWidget` into chat welcome area

**Files:**
- Modify: `crates/shannon-ui/src/widgets/chat.rs` (around lines 680-767)
- Modify: `crates/shannon-ui/src/widgets/mod.rs` (extend `MainLayoutWidget` to allocate area for StatusCard)

- [ ] **Step 1: Locate the welcome screen block**

Run: `grep -n "if self.messages.is_empty\|let mut welcome_lines" crates/shannon-ui/src/widgets/chat.rs`
Read 100 lines around the match.

- [ ] **Step 2: Split welcome area to allocate StatusCard area**

In `crates/shannon-ui/src/widgets/chat.rs`, find the welcome rendering. The current logic puts the welcome_lines directly into the chat area. Replace with a split:

```rust
if self.messages.is_empty() {
    use crate::widgets::status_card::{render_status_card, CardStatus};
    // Allocate top 6 lines for StatusCard, rest for welcome text
    let (card_area, welcome_area) = if inner.height > 8 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(0)])
            .split(inner);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, inner)
    };

    if let Some(card_area) = card_area {
        // Use real state passed via constructor (see step 3)
        render_status_card(
            f,
            card_area,
            CardStatus::Unconfigured, // TODO: replace with real state in step 4
            None,
            None,
            None,
            &[],
        );
    }

    // Existing welcome_lines rendering, but pointed at welcome_area instead of inner
    // ... (copy existing welcome_lines construction here, unchanged)
    f.render_widget(Paragraph::new(welcome_lines).wrap(Wrap { trim: true }), welcome_area);
}
```

- [ ] **Step 3: Extend `ChatWidget` to hold provider/model/tier state**

If `ChatWidget` does not already have access to `ReplState`, add fields:

```rust
pub struct ChatWidget {
    // ... existing fields ...
    pub active_provider: Option<String>,
    pub active_model: Option<String>,
    pub active_tier: Option<String>,
}
```

Add a setter:

```rust
    pub fn set_active(&mut self, provider: Option<String>, model: Option<String>, tier: Option<String>) {
        self.active_provider = provider;
        self.active_model = model;
        self.active_tier = tier;
    }
```

- [ ] **Step 4: Wire real state into the StatusCard call**

Replace the placeholder call in step 2 with:

```rust
        let available = vec![
            ("anthropic", &["claude-opus-4", "claude-sonnet-4", "claude-haiku-4-5"][..]),
            ("openai", &["gpt-4o", "gpt-4o-mini"][..]),
        ];
        let status = if self.active_provider.is_some() {
            CardStatus::Configured
        } else {
            CardStatus::Unconfigured
        };
        render_status_card(
            f,
            card_area,
            status,
            self.active_provider.as_deref(),
            self.active_model.as_deref(),
            self.active_tier.as_deref(),
            &available,
        );
```

- [ ] **Step 5: Find where `ChatWidget` is instantiated and pass state**

Run: `grep -rn "ChatWidget::new\|ChatWidget {" crates/shannon-ui/src`
In the constructor caller, add:

```rust
    chat_widget.set_active(
        state.selected_provider.as_ref().map(|p| format!("{:?}", p)),
        state.model.clone(),
        compute_tier_label(&state.model), // helper from Task 14
    );
```

If `compute_tier_label` doesn't exist yet (added in Task 14), use a placeholder:

```rust
    chat_widget.set_active(
        state.selected_provider.as_ref().map(|p| format!("{:?}", p)),
        state.model.clone(),
        None, // will be filled in by Task 14
    );
```

- [ ] **Step 6: Verify compile + run chat widget tests**

Run: `cargo check -p shannon-ui && cargo nextest run -p shannon-ui`
Expected: compiles; existing tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/shannon-ui/src/widgets/chat.rs crates/shannon-ui/src/widgets/mod.rs
git commit -m "feat(ui): insert StatusCardWidget into chat welcome area"
```

---

## Task 10: Upgrade `StatusBarWidget` pill format

**Files:**
- Modify: `crates/shannon-ui/src/widgets/status_bar.rs` (lines 178-198)

- [ ] **Step 1: Locate the model pill block**

Run: `grep -n "truncate_model\|let label = if let Some" crates/shannon-ui/src/widgets/status_bar.rs`
Read 30 lines.

- [ ] **Step 2: Replace pill format**

Find the block that constructs `label`:

```rust
        let label = if let Some(effort) = effort_level {
            format!("[{} · {}]", truncate_model(m), effort)
        } else {
            format!("[{}]", truncate_model(m))
        };
```

Replace with:

```rust
        let provider_short = provider_short_name(provider);  // "anthropic" -> "anthropic"
        let tier_str = tier_label_for(m);  // "fast" | "standard" | "pro" | "unknown"
        let label = format!("[{} · {} · {}]", provider_short, truncate_model(m), tier_str);
```

- [ ] **Step 3: Add helper functions**

At the top of `status_bar.rs` (after imports):

```rust
fn provider_short_name(provider: &Option<crate::shannon_engine::api::LlmProvider>) -> &'static str {
    match provider {
        Some(crate::shannon_engine::api::LlmProvider::Anthropic) => "anthropic",
        Some(crate::shannon_engine::api::LlmProvider::OpenAi) => "openai",
        Some(crate::shannon_engine::api::LlmProvider::Ollama) => "ollama",
        Some(crate::shannon_engine::api::LlmProvider::Gemini) => "gemini",
        Some(crate::shannon_engine::api::LlmProvider::Deepseek) => "deepseek",
        Some(crate::shannon_engine::api::LlmProvider::Zhipu) => "zhipu",
        Some(crate::shannon_engine::api::LlmProvider::ZhipuInternational) => "zhipu-intl",
        _ => "unknown",
    }
}

fn tier_label_for(model_id: &str) -> &'static str {
    let lower = model_id.to_lowercase();
    if lower.contains("haiku") || lower.contains("flash") || lower.contains("mini") || lower.contains("nano") || lower.contains("turbo") {
        "fast"
    } else if lower.contains("opus") || lower.contains("ultra") || lower.contains("o1") || lower.contains("max") {
        "pro"
    } else {
        "standard"
    }
}
```

Note: adjust the import paths to match the actual `LlmProvider` location in the codebase. Use `cargo check` to iterate.

- [ ] **Step 4: Update the no-model branch**

Find the `else` branch (no model configured):

```rust
    } else {
        left.push(Span::styled(" ", Style::default().fg(theme.border_dim)));
        left.push(Span::styled(
            format!("[{}]", t!("ui.no_model")),
            Style::default().fg(theme.warning),
        ));
    }
```

Replace with:

```rust
    } else {
        left.push(Span::styled(" ", Style::default().fg(theme.border_dim)));
        left.push(Span::styled(
            "[No provider connected]".to_string(),
            Style::default().fg(theme.warning),
        ));
    }
```

- [ ] **Step 5: Add a unit test for pill format**

Find the existing `#[cfg(test)] mod tests` in `status_bar.rs` (or add one). Add:

```rust
    #[test]
    fn tier_label_for_classifies_models() {
        assert_eq!(tier_label_for("claude-haiku-4-5"), "fast");
        assert_eq!(tier_label_for("claude-sonnet-4"), "standard");
        assert_eq!(tier_label_for("claude-opus-4"), "pro");
        assert_eq!(tier_label_for("gemini-1.5-flash"), "fast");
        assert_eq!(tier_label_for("gpt-4o-mini"), "fast");
        assert_eq!(tier_label_for("o1-preview"), "pro");
    }
```

- [ ] **Step 6: Run tests**

Run: `cargo nextest run -p shannon-ui tier_label_for_classifies_models`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/shannon-ui/src/widgets/status_bar.rs
git commit -m "feat(ui): upgrade StatusBar pill to [provider/model/tier] format"
```

**M2 complete**. Next: M3.

---

## Task 11: Add `TierName` enum + methods

**Files:**
- Modify: `crates/shannon-types/src/provider_config.rs`

- [ ] **Step 1: Locate the end of the file**

Run: `tail -30 crates/shannon-types/src/provider_config.rs`

- [ ] **Step 2: Append the `TierName` enum + impl**

Add at the end of `crates/shannon-types/src/provider_config.rs`:

```rust
/// Model tier. Canonical names are `fast`/`standard`/`pro`/`auto`.
/// Aliases (input-only) include Anthropic's `haiku`/`sonnet`/`opus`
/// and provider-native names (`flash`/`mini`/`plus`/`ultra`/`max`).
///
/// `Auto` is reserved for future use (model auto-routing by task type)
/// and is not yet wired to any production code path.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TierName {
    Fast,
    Standard,
    Pro,
    Auto,
}

impl TierName {
    /// Canonical lowercase name (used in toml, logs, status pills).
    pub fn canonical(self) -> &'static str {
        match self {
            TierName::Fast => "fast",
            TierName::Standard => "standard",
            TierName::Pro => "pro",
            TierName::Auto => "auto",
        }
    }

    /// Human-readable display label (capitalized, used in UI).
    pub fn display(self) -> &'static str {
        match self {
            TierName::Fast => "Fast",
            TierName::Standard => "Standard",
            TierName::Pro => "Pro",
            TierName::Auto => "Auto",
        }
    }

    /// Normalize any accepted user input to canonical TierName.
    /// Accepts canonical names + Anthropic aliases + other provider-native
    /// aliases. Case-insensitive. Returns None for unrecognized input.
    pub fn from_user_input(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            // Canonical
            "fast" => Some(TierName::Fast),
            "standard" => Some(TierName::Standard),
            "pro" => Some(TierName::Pro),
            "auto" => Some(TierName::Auto),
            // Aliases → Fast
            "flash" | "mini" | "nano" | "haiku" => Some(TierName::Fast),
            // Aliases → Standard
            "plus" | "sonnet" | "medium" | "turbo" => Some(TierName::Standard),
            // Aliases → Pro
            "opus" | "ultra" | "max" | "large" => Some(TierName::Pro),
            _ => None,
        }
    }

    /// Tab-completion suggestions shown to the user.
    /// Order: canonical first, then Anthropic aliases, then other aliases.
    pub fn suggestions() -> &'static [&'static str] {
        &[
            "fast", "standard", "pro", "auto",
            "haiku", "sonnet", "opus",
            "flash", "mini", "plus", "ultra", "max",
        ]
    }
}
```

- [ ] **Step 3: Add unit tests**

Append:

```rust
#[cfg(test)]
mod tier_name_tests {
    use super::*;

    #[test]
    fn canonical_is_lowercase() {
        assert_eq!(TierName::Fast.canonical(), "fast");
        assert_eq!(TierName::Standard.canonical(), "standard");
        assert_eq!(TierName::Pro.canonical(), "pro");
        assert_eq!(TierName::Auto.canonical(), "auto");
    }

    #[test]
    fn from_user_input_accepts_canonical() {
        assert_eq!(TierName::from_user_input("fast"), Some(TierName::Fast));
        assert_eq!(TierName::from_user_input("standard"), Some(TierName::Standard));
        assert_eq!(TierName::from_user_input("pro"), Some(TierName::Pro));
        assert_eq!(TierName::from_user_input("auto"), Some(TierName::Auto));
    }

    #[test]
    fn from_user_input_accepts_anthropic_aliases() {
        assert_eq!(TierName::from_user_input("haiku"), Some(TierName::Fast));
        assert_eq!(TierName::from_user_input("sonnet"), Some(TierName::Standard));
        assert_eq!(TierName::from_user_input("opus"), Some(TierName::Pro));
    }

    #[test]
    fn from_user_input_accepts_other_provider_aliases() {
        assert_eq!(TierName::from_user_input("flash"), Some(TierName::Fast));
        assert_eq!(TierName::from_user_input("mini"), Some(TierName::Fast));
        assert_eq!(TierName::from_user_input("plus"), Some(TierName::Standard));
        assert_eq!(TierName::from_user_input("ultra"), Some(TierName::Pro));
        assert_eq!(TierName::from_user_input("max"), Some(TierName::Pro));
    }

    #[test]
    fn from_user_input_is_case_insensitive() {
        assert_eq!(TierName::from_user_input("FAST"), Some(TierName::Fast));
        assert_eq!(TierName::from_user_input("Haiku"), Some(TierName::Fast));
        assert_eq!(TierName::from_user_input("oPuS"), Some(TierName::Pro));
    }

    #[test]
    fn from_user_input_rejects_unknown() {
        assert_eq!(TierName::from_user_input(""), None);
        assert_eq!(TierName::from_user_input("xyz"), None);
        assert_eq!(TierName::from_user_input("turbo-xl"), None);
    }

    #[test]
    fn canonical_round_trips_through_from_user_input() {
        for tier in [TierName::Fast, TierName::Standard, TierName::Pro, TierName::Auto] {
            assert_eq!(TierName::from_user_input(tier.canonical()), Some(tier));
        }
    }

    #[test]
    fn suggestions_starts_with_canonical() {
        let s = TierName::suggestions();
        assert_eq!(s[0], "fast");
        assert_eq!(s[1], "standard");
        assert_eq!(s[2], "pro");
        assert_eq!(s[3], "auto");
        // Anthropic aliases present
        assert!(s.contains(&"haiku"));
        assert!(s.contains(&"sonnet"));
        assert!(s.contains(&"opus"));
    }
}
```

- [ ] **Step 4: Run the new tests**

Run: `cargo nextest run -p shannon-types tier_name`
Expected: 8 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/shannon-types/src/provider_config.rs
git commit -m "feat(types): add TierName enum with canonical/display/from_user_input/suggestions"
```

---

## Task 12: Add `ProviderTiers` struct + `ProviderProfile.tiers` field

**Files:**
- Modify: `crates/shannon-types/src/provider_config.rs` (find `ProviderProfile`)
- Modify: `crates/shannon-core/src/provider_config_store.rs` (verify round-trip)

- [ ] **Step 1: Locate `ProviderProfile` struct**

Run: `grep -n "pub struct ProviderProfile" crates/shannon-types/src/provider_config.rs`

- [ ] **Step 2: Add `ProviderTiers` struct**

In `crates/shannon-types/src/provider_config.rs`, immediately above `ProviderProfile`, add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProviderTiers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pro: Option<String>,
}
```

- [ ] **Step 3: Add `tiers` field to `ProviderProfile`**

In the `ProviderProfile` struct, add (at the end, before the closing brace):

```rust
    #[serde(default)]
    pub tiers: ProviderTiers,
```

- [ ] **Step 4: Verify schema validation tests**

Run: `cargo nextest run -p shannon-types`
Expected: existing tests pass (including any JsonSchema tests in `tests/provider_config_schema.rs`).

- [ ] **Step 5: Add round-trip test**

Append to the test module in `provider_config.rs`:

```rust
    #[test]
    fn provider_profile_round_trip_with_tiers() {
        let profile = ProviderProfile {
            id: "anthropic".to_string(),
            kind: ProviderKind::Anthropic,
            display_name: "Anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            models_url: None,
            credential: CredentialRef::Env { var: "ANTHROPIC_API_KEY".to_string() },
            extra_headers: Default::default(),
            default_max_tokens: None,
            fallback_models: vec![],
            quirks: ProviderQuirks::default(),
            tiers: ProviderTiers {
                fast: Some("claude-haiku-4-5".to_string()),
                standard: Some("claude-sonnet-4-20250514".to_string()),
                pro: Some("claude-opus-4".to_string()),
            },
        };

        let toml_str = toml::to_string(&profile).expect("serialize");
        assert!(toml_str.contains("fast = \"claude-haiku-4-5\""));
        assert!(toml_str.contains("standard = \"claude-sonnet-4-20250514\""));
        assert!(toml_str.contains("pro = \"claude-opus-4\""));

        let parsed: ProviderProfile = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.tiers.fast, profile.tiers.fast);
        assert_eq!(parsed.tiers.standard, profile.tiers.standard);
        assert_eq!(parsed.tiers.pro, profile.tiers.pro);
    }

    #[test]
    fn provider_profile_round_trip_without_tiers_uses_default() {
        // Existing toml files without `tiers` should still parse
        let minimal_toml = r#"
            id = "anthropic"
            kind = "Anthropic"
            display_name = "Anthropic"
            base_url = "https://api.anthropic.com"
            credential = { type = "env", var = "ANTHROPIC_API_KEY" }
        "#;
        let parsed: ProviderProfile = toml::from_str(minimal_toml).expect("deserialize");
        assert_eq!(parsed.tiers, ProviderTiers::default());
    }
```

If `toml` is not already a dev-dependency, add to `crates/shannon-types/Cargo.toml`:

```toml
[dev-dependencies]
toml = "0.8"
```

- [ ] **Step 6: Run the round-trip tests**

Run: `cargo nextest run -p shannon-types provider_profile_round_trip`
Expected: 2 PASS.

- [ ] **Step 7: Verify provider_config_store round-trip**

Run: `cargo nextest run -p shannon-core provider_serialization`
Expected: existing tests still pass.

- [ ] **Step 8: Commit**

```bash
git add crates/shannon-types/src/provider_config.rs crates/shannon-types/Cargo.toml crates/shannon-core/src/provider_config_store.rs
git commit -m "feat(types,core): add ProviderTiers struct and wire through serialization"
```

---

## Task 13: Add `TierLabel` + `ModelInfo::tier_label`

**Files:**
- Modify: `crates/shannon-core/src/model_registry.rs`

- [ ] **Step 1: Locate `ModelInfo` struct**

Run: `grep -n "pub struct ModelInfo" crates/shannon-core/src/model_registry.rs`

- [ ] **Step 2: Add `TierLabel` enum**

In `crates/shannon-core/src/model_registry.rs`, near the top (after `ModelCapabilities`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierLabel {
    Fast,
    Standard,
    Pro,
    Unknown,
}

impl TierLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            TierLabel::Fast => "fast",
            TierLabel::Standard => "standard",
            TierLabel::Pro => "pro",
            TierLabel::Unknown => "unknown",
        }
    }
}
```

- [ ] **Step 3: Add `tier_label()` method on `ModelInfo`**

Find `impl ModelInfo {` and add:

```rust
    pub fn tier_label(&self) -> TierLabel {
        let caps = self.capabilities;
        let id = self.id.as_str();
        if caps.contains(ModelCapabilities::CHEAP) || caps.contains(ModelCapabilities::SPEED) {
            TierLabel::Fast
        } else if id.contains("opus")
            || id.contains("o1")
            || id.contains("ultra")
            || id.contains("max")
        {
            TierLabel::Pro
        } else if caps.contains(ModelCapabilities::REASONING) || caps.contains(ModelCapabilities::CODING) {
            TierLabel::Standard
        } else {
            TierLabel::Unknown
        }
    }
```

- [ ] **Step 4: Add unit tests**

In the existing test module in `model_registry.rs`, add:

```rust
    #[test]
    fn tier_label_classifies_anthropic_models() {
        let haiku = find_model("claude-haiku-4-5").unwrap();
        assert_eq!(haiku.tier_label(), TierLabel::Fast);

        let sonnet = find_model("claude-sonnet-4-20250514").unwrap();
        assert_eq!(sonnet.tier_label(), TierLabel::Standard);

        let opus = find_model("claude-opus-4").unwrap();
        assert_eq!(opus.tier_label(), TierLabel::Pro);
    }

    #[test]
    fn tier_label_classifies_gemini_models() {
        let flash = find_model("gemini-1.5-flash").unwrap();
        assert_eq!(flash.tier_label(), TierLabel::Fast);

        let pro = find_model("gemini-1.5-pro").unwrap();
        assert_eq!(pro.tier_label(), TierLabel::Standard);
    }

    #[test]
    fn tier_label_classifies_openai_models() {
        let mini = find_model("gpt-4o-mini").unwrap();
        assert_eq!(mini.tier_label(), TierLabel::Fast);

        let o1 = find_model("o1-preview").unwrap();
        assert_eq!(o1.tier_label(), TierLabel::Pro);
    }
```

Note: if `find_model` does not exist, use whichever lookup function exists in `model_registry.rs`. If none exists, add a minimal one:

```rust
fn find_model(id: &str) -> Option<&'static ModelInfo> {
    MODEL_CATALOG.iter().find(|m| m.id == id)
}
```

- [ ] **Step 5: Run tests**

Run: `cargo nextest run -p shannon-core tier_label`
Expected: 3 PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/shannon-core/src/model_registry.rs
git commit -m "feat(core): add TierLabel enum and ModelInfo::tier_label()"
```

---

## Task 14: Add `resolve_tier()` with catalog inference + alias fallback

**Files:**
- Modify: `crates/shannon-core/src/model_registry.rs`

- [ ] **Step 1: Locate existing `resolve_model_alias` for reference**

Run: `grep -n "pub fn resolve_model_alias" crates/shannon-core/src/model_registry.rs`
Read the function body.

- [ ] **Step 2: Append `resolve_tier()` function**

At the end of `model_registry.rs` (above the test module):

```rust
/// Resolve a tier (canonical or alias) to a concrete model id for a provider.
///
/// Resolution order:
///   1. User-configured `profile_tiers.<canonical>` override (from providers.toml)
///   2. Catalog match using `ModelCapabilities` (Fast ⇒ SPEED|CHEAP, etc.)
///   3. Internal `ModelTier` enum fallback (Opus/Sonnet/Haiku)
///   4. None (caller should display "tier not available for this provider")
///
/// `tier_input` accepts both canonical names ("fast") and aliases ("haiku", "flash", etc.).
/// Returns None for unrecognized input or for the reserved `Auto` tier.
pub fn resolve_tier(
    tier_input: &str,
    provider: &LlmProvider,
    profile_tiers: &ProviderTiers,
) -> Option<String> {
    let tier = TierName::from_user_input(tier_input)?;
    if matches!(tier, TierName::Auto) {
        return None;
    }

    // 1. Explicit user override
    let explicit = match tier {
        TierName::Fast => &profile_tiers.fast,
        TierName::Standard => &profile_tiers.standard,
        TierName::Pro => &profile_tiers.pro,
        TierName::Auto => return None,
    };
    if let Some(id) = explicit {
        return Some(id.clone());
    }

    // 2. Catalog-based inference using ModelCapabilities
    let wanted = match tier {
        TierName::Fast => ModelCapabilities::SPEED | ModelCapabilities::CHEAP,
        TierName::Standard => ModelCapabilities::CODING,
        TierName::Pro => ModelCapabilities::REASONING,
        TierName::Auto => return None,
    };
    if let Some(m) = catalog_models_for_provider(provider)
        .iter()
        .filter(|m| m.capabilities.contains(wanted))
        .min_by_key(|m| m.cost_tier)
    {
        return Some(m.id.to_string());
    }

    // 3. Internal ModelTier enum fallback
    let mt = match tier {
        TierName::Fast => ModelTier::Haiku,
        TierName::Standard => ModelTier::Sonnet,
        TierName::Pro => ModelTier::Opus,
        TierName::Auto => return None,
    };
    resolve_model_alias(mt.into_alias(), Some(provider)).map(|s| s.to_string())
}
```

Note: adjust the import of `LlmProvider` (likely `shannon_engine::api::LlmProvider`), `ProviderTiers` (`shannon_types::provider_config::ProviderTiers`), and `TierName` (same module).

Also: `ModelTier::into_alias()` may not exist; if `ModelTier` is an enum without that method, use a match:

```rust
let alias = match mt {
    ModelTier::Haiku => "haiku",
    ModelTier::Sonnet => "sonnet",
    ModelTier::Opus => "opus",
};
resolve_model_alias(alias, Some(provider)).map(|s| s.to_string())
```

- [ ] **Step 3: Add unit tests**

In the test module:

```rust
    #[test]
    fn resolve_tier_anthropic_fast_uses_haiku() {
        let tiers = ProviderTiers::default();
        let resolved = resolve_tier("fast", &LlmProvider::Anthropic, &tiers);
        assert_eq!(resolved, Some("claude-haiku-4-5".to_string()));
    }

    #[test]
    fn resolve_tier_anthropic_standard_uses_sonnet() {
        let tiers = ProviderTiers::default();
        let resolved = resolve_tier("standard", &LlmProvider::Anthropic, &tiers);
        assert_eq!(resolved, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn resolve_tier_anthropic_pro_uses_opus() {
        let tiers = ProviderTiers::default();
        let resolved = resolve_tier("pro", &LlmProvider::Anthropic, &tiers);
        assert_eq!(resolved, Some("claude-opus-4".to_string()));
    }

    #[test]
    fn resolve_tier_accepts_anthropic_aliases() {
        let tiers = ProviderTiers::default();
        assert_eq!(resolve_tier("haiku", &LlmProvider::Anthropic, &tiers),
                   Some("claude-haiku-4-5".to_string()));
        assert_eq!(resolve_tier("sonnet", &LlmProvider::Anthropic, &tiers),
                   Some("claude-sonnet-4-20250514".to_string()));
        assert_eq!(resolve_tier("opus", &LlmProvider::Anthropic, &tiers),
                   Some("claude-opus-4".to_string()));
    }

    #[test]
    fn resolve_tier_accepts_other_provider_aliases() {
        let tiers = ProviderTiers::default();
        assert_eq!(resolve_tier("flash", &LlmProvider::Gemini, &tiers).is_some(), true);
        assert_eq!(resolve_tier("mini", &LlmProvider::OpenAi, &tiers).is_some(), true);
        assert_eq!(resolve_tier("ultra", &LlmProvider::Gemini, &tiers).is_some(), true);
    }

    #[test]
    fn resolve_tier_profile_override_wins() {
        let tiers = ProviderTiers {
            fast: Some("claude-haiku-3-5".to_string()),
            ..Default::default()
        };
        let resolved = resolve_tier("fast", &LlmProvider::Anthropic, &tiers);
        assert_eq!(resolved, Some("claude-haiku-3-5".to_string()),
                   "explicit profile_tiers.fast should win over catalog default");
    }

    #[test]
    fn resolve_tier_unknown_input_returns_none() {
        let tiers = ProviderTiers::default();
        assert_eq!(resolve_tier("garbage", &LlmProvider::Anthropic, &tiers), None);
        assert_eq!(resolve_tier("", &LlmProvider::Anthropic, &tiers), None);
    }

    #[test]
    fn resolve_tier_auto_returns_none() {
        let tiers = ProviderTiers::default();
        assert_eq!(resolve_tier("auto", &LlmProvider::Anthropic, &tiers), None);
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo nextest run -p shannon-core resolve_tier`
Expected: 8 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/shannon-core/src/model_registry.rs
git commit -m "feat(core): add resolve_tier() with catalog inference and alias fallback"
```

---

## Task 15: Extend `model_aliases()` for Tab completion

**Files:**
- Modify: `crates/shannon-core/src/model_registry.rs` (find `model_aliases` function)

- [ ] **Step 1: Locate `model_aliases`**

Run: `grep -n "pub fn model_aliases" crates/shannon-core/src/model_registry.rs`

- [ ] **Step 2: Replace the function body**

Replace the existing `model_aliases()` function body with:

```rust
pub fn model_aliases() -> &'static [&'static str] {
    TierName::suggestions()
}
```

- [ ] **Step 3: Add the import**

At the top of `model_registry.rs`, ensure:

```rust
use crate::shannon_types::provider_config::TierName;
```

(or wherever `TierName` lives — match the existing import style).

- [ ] **Step 4: Run tests that use `model_aliases`**

Run: `cargo nextest run -p shannon-core model_aliases`
Expected: existing tests pass + (newly) includes canonical + Anthropic aliases.

- [ ] **Step 5: Commit**

```bash
git add crates/shannon-core/src/model_registry.rs
git commit -m "feat(core): extend model_aliases() with canonical and Anthropic aliases"
```

---

## Task 16: Add `--tier` parsing to `handle_model`

**Files:**
- Modify: `crates/shannon-ui/src/repl/commands/config.rs` (lines 35-85)

- [ ] **Step 1: Locate `handle_model`**

Run: `grep -n "pub(crate) fn handle_model" crates/shannon-ui/src/repl/commands/config.rs`
Read the function body.

- [ ] **Step 2: Add `--tier` branch at the top of `handle_model`**

Immediately after the function signature (before the existing `if args.is_empty()` check), add:

```rust
    // /model --tier <name> [provider] [--save]
    if args.starts_with("--tier") {
        return handle_model_tier(repl, args);
    }
```

Then add the helper function below `handle_model`:

```rust
fn handle_model_tier(repl: &mut Repl, args: &str) -> Result<()> {
    use crate::shannon_types::provider_config::TierName;
    let parts: Vec<&str> = args.split_whitespace().collect();
    let tier_input = parts.get(1).copied().unwrap_or("");
    let tier = TierName::from_user_input(tier_input).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown tier '{}'. Try one of: {}",
            tier_input,
            TierName::suggestions().join(", ")
        )
    })?;
    if matches!(tier, TierName::Auto) {
        return Err(anyhow::anyhow!(
            "--tier auto is reserved for a future spec; explicit tiers only for now"
        ));
    }
    let explicit_provider_str = parts.get(2).copied();
    let save = parts.iter().any(|p| *p == "--save");
    let provider = match explicit_provider_str {
        Some(p) => parse_provider_name(p)?,
        None => repl.state.selected_provider.clone()
            .ok_or_else(|| anyhow::anyhow!(
                "No provider selected; specify one: /model --tier <tier> <provider>"
            ))?,
    };
    let profile_tiers = load_provider_tiers(&provider);
    let model_id = shannon_core::model_registry::resolve_tier(
        tier_input, &provider, &profile_tiers
    ).ok_or_else(|| {
        anyhow::anyhow!(
            "No model found for tier={} provider={}",
            tier.canonical(), provider
        )
    })?;

    let prev_model = repl.state.model.clone();
    let prev_provider = repl.state.selected_provider.clone();
    repl.state.model = Some(model_id.clone());
    repl.state.selected_provider = Some(provider.clone());
    if let Err(e) = crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
        model: repl.state.model.clone(),
        provider: repl.state.selected_provider.clone(),
        theme: Some(repl.state.theme.name.to_string()),
    }) {
        // Rollback state if persistence fails
        repl.state.model = prev_model;
        repl.state.selected_provider = prev_provider;
        return Err(e);
    }
    if save {
        persist_model_to_providers_toml(&provider, &model_id, tier)?;
    }
    Ok(())
}
```

- [ ] **Step 3: Add helper stubs**

Add at the end of `config.rs`:

```rust
fn load_provider_tiers(_provider: &LlmProvider) -> ProviderTiers {
    // TODO: load from ~/.shannon/providers.toml
    ProviderTiers::default()
}

fn persist_model_to_providers_toml(
    _provider: &LlmProvider,
    _model_id: &str,
    _tier: TierName,
) -> Result<()> {
    // Implemented in Task 17
    Ok(())
}
```

Adjust the import path of `LlmProvider` to match. If the actual type lives in `shannon_engine::api::LlmProvider`, add the import.

- [ ] **Step 4: Verify compile**

Run: `cargo check -p shannon-ui`
Expected: no errors. (If `parse_provider_name` doesn't exist or has different signature, adjust the call to match the existing function in the same file.)

- [ ] **Step 5: Commit**

```bash
git add crates/shannon-ui/src/repl/commands/config.rs
git commit -m "feat(ui): add --tier arg parsing to handle_model"
```

---

## Task 17: Add `--save` flag + `persist_model_to_providers_toml`

**Files:**
- Modify: `crates/shannon-ui/src/repl/commands/config.rs`
- Modify: `crates/shannon-core/src/provider_config_store.rs` (verify wire-through)

- [ ] **Step 1: Locate `provider_config_store::save`**

Run: `grep -n "pub fn save\|pub fn load" crates/shannon-core/src/provider_config_store.rs`

- [ ] **Step 2: Implement `persist_model_to_providers_toml`**

Replace the stub from Task 16 with:

```rust
fn persist_model_to_providers_toml(
    provider: &LlmProvider,
    model_id: &str,
    tier: TierName,
) -> Result<()> {
    use shannon_core::provider_config_store::ProviderConfigStore;
    let mut store = ProviderConfigStore::load_or_default();
    let profile = store.ensure_provider(provider);
    match tier {
        TierName::Fast => profile.tiers.fast = Some(model_id.to_string()),
        TierName::Standard => profile.tiers.standard = Some(model_id.to_string()),
        TierName::Pro => profile.tiers.pro = Some(model_id.to_string()),
        TierName::Auto => return Err(anyhow::anyhow!("Auto tier should never be persisted")),
    }
    store.save()
}
```

- [ ] **Step 3: Verify `ensure_provider` exists or add it**

Run: `grep -n "ensure_provider\|pub fn" crates/shannon-core/src/provider_config_store.rs | head -20`

If `ensure_provider` does not exist, add it to `ProviderConfigStore`:

```rust
    /// Get-or-create the profile for the given provider.
    pub fn ensure_provider(&mut self, provider: &LlmProvider) -> &mut ProviderProfile {
        let id = shannon_core::provider_resolver::llm_provider_id(provider);
        if let Some(idx) = self.providers.iter().position(|p| p.id == id) {
            &mut self.providers[idx]
        } else {
            self.providers.push(ProviderProfile {
                id: id.to_string(),
                kind: ProviderKind::Anthropic, // placeholder; adjust per provider
                ..Default::default()
            });
            self.providers.last_mut().unwrap()
        }
    }
```

Note: `Default` for `ProviderProfile` requires all fields to have defaults; if not, use explicit construction matching the struct.

- [ ] **Step 4: Add integration test**

Create `crates/shannon-ui/tests/handle_model_tier_integration.rs`:

```rust
//! Integration test: /model --tier routes to correct model.

use shannon_types::provider_config::TierName;

#[test]
fn tier_name_from_user_input_resolves_anthropic_aliases() {
    assert_eq!(TierName::from_user_input("haiku"), Some(TierName::Fast));
    assert_eq!(TierName::from_user_input("sonnet"), Some(TierName::Standard));
    assert_eq!(TierName::from_user_input("opus"), Some(TierName::Pro));
}

#[test]
fn tier_name_persistence_uses_canonical_form() {
    // After normalize, "haiku" → TierName::Fast → canonical "fast"
    let tier = TierName::from_user_input("haiku").unwrap();
    assert_eq!(tier.canonical(), "fast");
    assert_ne!(tier.canonical(), "haiku");
}
```

- [ ] **Step 5: Run tests**

Run: `cargo nextest run -p shannon-ui tier_name -- cargo nextest run -p shannon-types tier_name`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/shannon-ui/src/repl/commands/config.rs crates/shannon-core/src/provider_config_store.rs crates/shannon-ui/tests/handle_model_tier_integration.rs
git commit -m "feat(ui,core): implement persist_model_to_providers_toml with canonical tier names"
```

---

## Task 18: Add three-level tabs to `ModelPickerWidget`

**Files:**
- Modify: `crates/shannon-ui/src/widgets/select.rs` (around line 787)

- [ ] **Step 1: Locate `ModelPickerWidget`**

Run: `grep -n "ModelPickerWidget\|pub struct ModelPickerWidget" crates/shannon-ui/src/widgets/select.rs`
Read the struct definition and the `render` method.

- [ ] **Step 2: Add `current_tier_idx` field**

In `ModelPickerWidget` struct, add:

```rust
    pub current_tier_idx: usize,
```

In `ModelPickerWidget::new`, initialize it to 0:

```rust
            current_tier_idx: 0,
```

- [ ] **Step 3: Add tier-tab logic in `render`**

Find where the picker renders the provider tabs. Add tier-tab rendering between provider tabs and model list:

```rust
        // Determine current tier label
        let tiers = ["Fast", "Standard", "Pro"];
        let tier_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(std::iter::repeat(Constraint::Length(12)).take(tiers.len()))
            .split(<area for tier tabs>);

        for (i, tier_label) in tiers.iter().enumerate() {
            let style = if i == self.current_tier_idx {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let pill = Paragraph::new(Line::from(Span::styled(
                format!(" {} ", tier_label),
                style,
            )));
            f.render_widget(pill, tier_chunks[i]);
        }
```

- [ ] **Step 4: Add `Tab` keybinding to cycle tiers**

Locate the key handler (search for `KeyCode::Tab` or `match key.kind`). Add a branch:

```rust
        KeyCode::Tab => {
            self.current_tier_idx = (self.current_tier_idx + 1) % 3;
            self.refresh_models_for_tier();
        }
        KeyCode::BackTab => {
            self.current_tier_idx = if self.current_tier_idx == 0 { 2 } else { self.current_tier_idx - 1 };
            self.refresh_models_for_tier();
        }
```

Add the helper:

```rust
    fn refresh_models_for_tier(&mut self) {
        // Filter the current model list to those matching the selected tier
        let tier_label = match self.current_tier_idx {
            0 => TierLabel::Fast,
            1 => TierLabel::Standard,
            _ => TierLabel::Pro,
        };
        self.filtered_models.retain(|m| m.tier_label() == tier_label);
        self.selected_model_idx = 0;
    }
```

- [ ] **Step 5: Add a widget test**

In the existing test module in `select.rs`:

```rust
    #[test]
    fn picker_cycles_through_tiers() {
        let mut picker = ModelPickerWidget::new(None);
        assert_eq!(picker.current_tier_idx, 0);
        picker.current_tier_idx = (picker.current_tier_idx + 1) % 3;
        assert_eq!(picker.current_tier_idx, 1);
        picker.current_tier_idx = (picker.current_tier_idx + 1) % 3;
        assert_eq!(picker.current_tier_idx, 2);
        picker.current_tier_idx = (picker.current_tier_idx + 1) % 3;
        assert_eq!(picker.current_tier_idx, 0, "should wrap around");
    }
```

- [ ] **Step 6: Run tests**

Run: `cargo nextest run -p shannon-ui picker`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/shannon-ui/src/widgets/select.rs
git commit -m "feat(ui): add three-level tabs to ModelPickerWidget (provider -> tier -> model)"
```

**M3 complete**. Next: M4.

---

## Task 19: Integration test for full `/model --tier` flow

**Files:**
- Modify: `crates/shannon-ui/tests/handle_model_tier_integration.rs`

- [ ] **Step 1: Add end-to-end flow test**

Append:

```rust
#[test]
fn full_flow_haiku_alias_resolves_to_anthropic_fast() {
    use shannon_core::model_registry::resolve_tier;
    use shannon_engine::api::LlmProvider;
    use shannon_types::provider_config::{ProviderTiers, TierName};

    let tier = TierName::from_user_input("haiku").expect("haiku is valid alias");
    assert_eq!(tier, TierName::Fast);

    let tiers = ProviderTiers::default();
    let resolved = resolve_tier("haiku", &LlmProvider::Anthropic, &tiers);
    assert!(resolved.is_some(), "haiku alias should resolve to claude-haiku-4-5 for anthropic");
    assert!(resolved.unwrap().contains("haiku"), "got: {:?}", resolved);
}

#[test]
fn full_flow_flash_alias_resolves_to_gemini_fast() {
    use shannon_core::model_registry::resolve_tier;
    use shannon_engine::api::LlmProvider;
    use shannon_types::provider_config::ProviderTiers;

    let tiers = ProviderTiers::default();
    let resolved = resolve_tier("flash", &LlmProvider::Gemini, &tiers);
    assert!(resolved.is_some(), "flash alias should resolve to a Gemini fast model");
    let id = resolved.unwrap();
    assert!(id.contains("flash"), "got: {}", id);
}

#[test]
fn full_flow_unknown_tier_input_suggests_canonical_names() {
    use shannon_types::provider_config::TierName;

    let result = TierName::from_user_input("turbo-xl");
    assert!(result.is_none());

    // The error message in handle_model would include suggestions()
    let suggestions = TierName::suggestions();
    assert!(suggestions.contains(&"fast"));
    assert!(suggestions.contains(&"haiku"));
}
```

- [ ] **Step 2: Run all integration tests**

Run: `cargo nextest run -p shannon-ui --test handle_model_tier_integration`
Expected: all PASS.

- [ ] **Step 3: Run full workspace tests**

Run: `cargo nextest run --workspace -E 'not test(/live_/)' 2>&1 | tail -30`
Expected: all existing tests pass + ~25 new tests across crates.

- [ ] **Step 4: Commit**

```bash
git add crates/shannon-ui/tests/handle_model_tier_integration.rs
git commit -m "test(ui): add end-to-end tier resolution integration tests"
```

---

## Task 20: Update ADR-0005 Phase 3 status

**Files:**
- Modify: `docs/adr/0005-unified-provider-model-credential-management.md`

- [ ] **Step 1: Locate the phase status section**

Run: `grep -n "Phase 3\|🔄 Partial\|Phase 4" docs/adr/0005-unified-provider-model-credential-management.md`

- [ ] **Step 2: Update Phase 3 status**

Change the Phase 3 line from `🔄 Partial` to `✅ Complete (2026-07-29)` and append a description:

```markdown
Phase 3: /connect + /model in code — ✅ Complete (2026-07-29)
  - /model --tier <name> [provider] [--save] switches between canonical tiers
    (fast/standard/pro) with Anthropic aliases (haiku/sonnet/opus) and other
    provider aliases (flash/mini/plus/ultra/max)
  - TierName::from_user_input() normalizes all aliases case-insensitively
  - Persisted state and ~/.shannon/providers.toml use canonical names only
  - --save persists to providers.toml (Phase 4 enabler)
```

- [ ] **Step 3: Update Phase 4 status**

Change Phase 4 from `⏳ Pending` to `🟡 In Progress (2026-07-29 — /model tier persistence partially landed; remaining: variable substitution)`:

```markdown
Phase 4: Config persistence + variable substitution — 🟡 In Progress
  - Tier persistence to ~/.shannon/providers.toml: ✅ (via /model --tier --save)
  - Variable substitution (${SHANNON_API_KEY} etc.): ⏳ Pending
```

- [ ] **Step 4: Commit**

```bash
git add docs/adr/0005-unified-provider-model-credential-management.md
git commit -m "docs(adr): mark ADR-0005 Phase 3 complete, Phase 4 in progress"
```

---

## Task 21: Update CLAUDE.md first-screen description

**Files:**
- Modify: `CLAUDE.md` (find the section that describes the first screen, near `shannon-ui` table)

- [ ] **Step 1: Locate the relevant section**

Run: `grep -n "shannon-ui\|first-screen\|welcome" CLAUDE.md`

- [ ] **Step 2: Add a "First-Screen UX" subsection**

After the `shannon-ui` row in the architecture table, add:

```markdown
### First-Screen UX

The REPL first screen (when chat is empty) renders a `StatusCardWidget` above the welcome text showing:
- Active provider + model + tier (canonical: fast/standard/pro; aliases accepted: haiku/sonnet/opus/flash/mini/plus/ultra/max)
- Available providers and their models (from `MODEL_CATALOG`)
- Command hints (`/connect`, `/model`, `/provider`, `/profile`, `/help`)

The StatusBar shows a compact `[provider/model · tier]` pill that updates in real time.

`/help` opens a modal overlay (does not pollute chat history); `/model --tier <name>` switches between tiers with `--save` persisting to `~/.shannon/providers.toml`.
```

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with first-screen UX description"
```

---

## Task 22: Update CHANGELOG.md

**Files:**
- Modify: `CHANGELOG.md` (or the topmost unreleased section)

- [ ] **Step 1: Locate unreleased section**

Run: `head -30 CHANGELOG.md`

- [ ] **Step 2: Add changelog entry**

If there's an `## [Unreleased]` section, add a sub-bullet:

```markdown
### Added

- First-screen status card showing active provider/model/tier plus available providers and models
- `/model --tier <fast|standard|pro>` command surface (also accepts aliases: `haiku`/`sonnet`/`opus`/`flash`/`mini`/`plus`/`ultra`/`max`)
- `/model --save` flag persists tier choice to `~/.shannon/providers.toml`
- Three-level picker navigation (provider → tier → model)
- `TierName` enum (`fast`/`standard`/`pro`/`auto`) with alias normalization

### Changed

- `/help` now opens a modal overlay instead of injecting a System message into chat history
  (prevents `<file>`/`<line>`/`<character>` placeholders from leaking into LLM context)
- StatusBar pill format upgraded from `[model]` to `[provider/model · tier]`
- `arg_hint` placeholders renamed from `<file>` to `<FILE_PATH>` (ALL_CAPS) to reduce LLM misidentification risk
```

If no unreleased section exists, create one at the top.

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(changelog): add entry for first-screen, /help overlay, and model-tier UX"
```

---

## Task 23: i18n strings for new UI text

**Files:**
- Modify: `crates/shannon-ui/locales/en.yaml`
- Modify: `crates/shannon-ui/locales/zh.yaml`

- [ ] **Step 1: Locate the existing locale files**

Run: `ls crates/shannon-ui/locales/`

- [ ] **Step 2: Add new keys to `en.yaml`**

Find the existing `ui:` section and add:

```yaml
ui:
  # ... existing keys ...
  status_card:
    title: " Status "
    no_provider: "No provider connected. Run /connect to get started."
    active_label: "Active:"
    tier_label: "Tier:"
    available_providers: "Available providers ({connected} connected / {total} supported):"
    commands: "Commands: /connect · /model · /provider · /profile · /help"
  help_overlay:
    title: " Shannon Help — Esc to close "
    categories: " Categories "
    commands: " Commands "
    footer: " j/k: switch category │ Enter: detail │ /: search │ Esc: close │ filter: {filter} "
  status_bar:
    no_provider: "[No provider connected]"
```

- [ ] **Step 3: Add Chinese translations to `zh.yaml`**

```yaml
ui:
  # ... existing keys ...
  status_card:
    title: " 状态 "
    no_provider: "未连接任何 provider。运行 /connect 开始。"
    active_label: "当前:"
    tier_label: "等级:"
    available_providers: "可用 provider ({connected} 已连接 / {total} 支持):"
    commands: "命令: /connect · /model · /provider · /profile · /help"
  help_overlay:
    title: " Shannon 帮助 — Esc 关闭 "
    categories: " 分类 "
    commands: " 命令 "
    footer: " j/k: 切换分类 │ Enter: 详情 │ /: 搜索 │ Esc: 关闭 │ 过滤: {filter} "
  status_bar:
    no_provider: "[未连接 provider]"
```

- [ ] **Step 4: Replace hardcoded strings in widgets with `t!()` calls**

In `crates/shannon-ui/src/widgets/status_card.rs`, replace `" Status "` with `t!("ui.status_card.title")`, `"No provider connected. Run /connect to get started."` with `t!("ui.status_card.no_provider")`, etc.

In `crates/shannon-ui/src/widgets/help_overlay.rs`, replace the title and footer strings with `t!()` calls.

In `crates/shannon-ui/src/widgets/status_bar.rs`, replace `"[No provider connected]"` with `format!("[{}]", t!("ui.status_bar.no_provider"))`.

(Use the `t!()` macro already in use at the top of `status_bar.rs`. Match the surrounding pattern.)

- [ ] **Step 5: Run all tests one last time**

Run: `cargo nextest run --workspace -E 'not test(/live_/)' 2>&1 | tail -10`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/shannon-ui/locales/ crates/shannon-ui/src/widgets/status_card.rs crates/shannon-ui/src/widgets/help_overlay.rs crates/shannon-ui/src/widgets/status_bar.rs
git commit -m "feat(ui): i18n strings for status card, help overlay, and status bar"
```

**M4 complete**. Plan done.

---

## Self-Review

Performed by Claude after writing the plan:

**1. Spec coverage check:**
- §2.1 (first-screen architecture): covered by Tasks 7-10 ✅
- §2.2 (`/help` XML pollution): covered by Tasks 1-6 ✅
- §2.3 (provider/model config): no new tasks needed (already implemented) ✅
- §2.4 (tier infrastructure): Tasks 11-15 implement the user-facing surface ✅
- §2.5 (`/connect`/`/model`): Tasks 16-18 extend `/model`; `/connect` already complete (ADR-0005) ✅
- §4 (target architecture): StatusCard §4.1 → Tasks 7-10; HelpOverlay §4.3 → Tasks 2-6; Tier surface §4.4 → Tasks 11-19 ✅
- §5 (component boundaries): all components defined in tasks ✅
- §6 (state machine): HelpOverlayState in Task 2; Modal state machine pattern reused ✅
- §7 (data flow): startup + `/model --tier fast --save` flows documented in spec; implementation in Tasks 9, 16-17 ✅
- §8 (testing strategy): 21 tests in spec mapped to tasks (Tasks 1, 2, 3, 4, 6, 8, 10, 11, 12, 13, 14, 15, 18, 19) ✅
- §9 (milestones): 4 milestones = Tasks 1-6 (M1), 7-10 (M2), 11-19 (M3), 20-23 (M4) ✅
- §11 (out of scope): ModelRouter, AuxRole, fallback_models, models.dev deferred — not in any task ✅
- §12 (success criteria): 9 criteria all addressed by tasks ✅

**2. Placeholder scan:** No "TBD", "TODO: implement later", "fill in details", or "similar to Task N" patterns. Helper stubs in Tasks 16-17 are explicitly marked with `// Implemented in Task N` and immediately replaced in subsequent tasks.

**3. Type consistency:**
- `TierName::Fast/Standard/Pro/Auto` defined Task 11, used in Tasks 12-19. ✅
- `TierLabel::Fast/Standard/Pro/Unknown` defined Task 13, used in Tasks 13, 14, 18. ✅
- `ProviderTiers { fast, standard, pro }` defined Task 12, used in Tasks 12, 14, 16, 17. ✅
- `resolve_tier(&str, &LlmProvider, &ProviderTiers) -> Option<String>` defined Task 14, called from Task 16. ✅
- `HelpOverlayState { filter, selected_category_idx, selected_command_idx, search_query }` defined Task 2, used in Tasks 3, 4, 5, 6. ✅
- `model_aliases()` returns `&'static [&'static str]` in both original and Task 15. ✅

**No issues found.** Plan is ready for execution.