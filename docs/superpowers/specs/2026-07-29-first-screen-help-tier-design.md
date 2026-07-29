# Shannon First-Screen, /help, and Model-Tier UX Design

- **Status**: Draft (awaiting user review)
- **Date**: 2026-07-29
- **Author**: Senior PM + Senior Architect review
- **Related ADR**: ADR-0005 (unified-provider-model-credential-management), Phase 3 (🔄 Partial) and Phase 4 (⏳ Pending)
- **Target branch**: `feat/unified-provider-model-mgmt`

---

## 1. Background & User-Reported Pain Points

User observed three issues in the current Shannon REPL:

1. **First screen lacks information** — after launching `shannon`, the user sees a static brand banner + three "Try asking" examples, but no indication of which provider/model is currently active, no tier classification of that model (flash/standard/pro or haiku/sonnet/opus), no overview of available providers, and no command hints for `/connect` or `/model`.
2. **`/help` output is unreadable** — the output contains XML-like tags (`<file>`, `<line>`, `<character>`, `<start_line>`, `<start_char>`, `<end_line>`, `<end_char>`) interleaved with control characters that look like noise rather than documentation.
3. **No model-tier UX** — there is no user-visible mechanism to switch between fast/standard/pro models, even though the underlying registry (`ModelTier`, `ModelCapabilities`, `ModelRouter::recommend`) has already implemented tier-aware logic internally.

These three issues are connected: a poor first-screen prevents users from understanding what the system has connected, the `/help` chaos breaks trust in command discovery, and the missing tier UX forces users into committing to one model for the whole session.

---

## 2. Investigation Findings

The following facts were verified by reading the source code (paths in `crates/`).

### 2.1 First-Screen Architecture

| Region | File:Line | Currently Shows | Missing |
|---|---|---|---|
| Header | `crates/shannon-ui/src/widgets/header.rs:16-87` | Brand + shortcuts (3 lines) | provider, model, tier |
| Chat welcome | `crates/shannon-ui/src/widgets/chat.rs:680-767` | Static logo + Try-asking examples + 3 slash hints | provider, model, tier, provider/model catalog |
| StatusBar | `crates/shannon-ui/src/widgets/status_bar.rs:178-198` | `[model_name]` pill (24-col truncated) | provider, tier label |
| OSC 0 title | `crates/shannon-ui/src/repl/render.rs:154-165` | `Shannon — {model_short} — {status}` | provider |

**Confirmed absence**: A full grep of `crates/shannon-ui/src/widgets/` shows **no widget renders `state.selected_provider: Option<LlmProvider>`** in the permanent UI. The provider field only surfaces in the `/model` picker (`widgets/select.rs:787`) as a transient popup.

The `SidebarWidget` is disabled (`widgets/sidebar.rs:3-5` — `// Currently disabled`), so even the legacy SidebarInfo path is not used.

### 2.2 `/help` XML-Tag Pollution Root Cause

The tags are not from `/help`'s own output — they leak through a two-stage pipeline:

**Stage 1**: `/help` writes the markdown text (containing `<file>`, `<line>`, `<character>` literals) into `repl.chat.messages` as a `ChatRole::System` message.

```rust
// crates/shannon-ui/src/repl/commands/mod.rs:554-566
fn handle_help(repl: &mut Repl, args: &str) -> Result<()> {
    let help_text = shannon_commands::help_utils::generate_help(Some(args));
    repl.chat.add_message(ChatRole::System, help_text);   // ← Stage 1: pollution
    Ok(())
}
```

**Stage 2**: On the next user turn, `query_engine/engine.rs:1293` injects the full chat history plus `tools.to_tool_definitions()` (which contains JSON Schema with `file_path`/`line`/`character` fields in `crates/shannon-tools/src/lsp.rs:1034-1052, 1178-1199, 1335-1350, 1462-1480, 2147`). The LLM sees both the markdown `<file>` literals and the JSON Schema `file_path` field, decides they are tool-argument placeholders, and reformats them into the user-visible response.

The arg_hint literals live in `crates/shannon-commands/src/builtin/help.rs:843, 854, 865, 876, 898, 909, 1054` (7 places, all LSP-tool related):

```rust
.with_arg_hint("<file> <line> <character>")              // go_to_definition
.with_arg_hint("<file> <line> <character>")              // find_references
.with_arg_hint("<file> <line> <character>")              // hover
.with_arg_hint("<file>")                                 // document_symbol
.with_arg_hint("<file> <line> <character> <new_name>")   // rename_symbol
.with_arg_hint("<file> <start_line> <start_char>...")    // code_actions
```

### 2.3 Provider/Model Configuration Status

- 25 `LlmProvider` enum variants (`shannon-engine/src/api/types.rs:49-103`)
- 6 `ProviderKind` wire-protocol variants (`shannon-types/src/provider_config.rs:20-29`)
- Config priority (verified in `ConfigBuilder::build()` at `shannon-core/src/unified_config.rs:285-307`):
  ```
  CLI flags > ~/.shannon/providers.toml (connected) > SHANNON_* env > .shannon.toml > ~/.shannon/config.toml > defaults
  ```
- API keys: `~/.shannon/credentials/<service>.json` (0600 plaintext) — ADR-0005
- Inline key `/connect <provider> <key>` gets redacted to `***` via `redact_secret_command` (`shannon-ui/src/repl/commands/mod.rs:99`)

### 2.4 Model-Tier Infrastructure (Already Implemented)

| Concept | Location | State |
|---|---|---|
| `ModelCapabilities` (REASONING/CODING/SPEED/CHEAP/VISION) | `model_registry.rs:14-49` | ✅ Implemented |
| `MODEL_CATALOG` (~50 static models) | `model_registry.rs:78-589` | ✅ Implemented |
| `ModelTier { Opus, Sonnet, Haiku }` | `model_registry.rs:802-806` | ✅ Implemented (internal use) |
| `EffortLevel { Low, Medium, High }` | `model_registry.rs:873-913` | ✅ Implemented |
| `TaskType { QuickQuery, CodeGeneration, ArchitectureDesign, ComplexWorkflow }` | `model_registry.rs:917-927` | ✅ Implemented (no caller) |
| `resolve_model_alias("opus/sonnet/haiku/fast/mini")` | `model_registry.rs:764` | ✅ Implemented |
| `ModelRouter::recommend()` / `recommend_fast()` | `model_registry.rs:930-986` | ✅ Implemented (zero callers) |
| `AuxRole` + `ModelProfile.auxiliary` HashMap | `provider_config.rs:54-60, 178-180` | ✅ Schema; **zero readers** |
| `ProviderProfile.fallback_models` | `provider_config.rs:138` | ✅ Schema; **zero readers** |
| `ProviderProfile.tiers` field | — | ❌ Not defined |
| `/model --tier` arg parsing | `commands/config.rs:35-85` | ❌ Not implemented |
| `ModelRouter` wired into `QueryEngine` | — | ❌ Not implemented |

**The infrastructure is 80% there — only the user-facing surface is missing.**

### 2.5 `/connect` and `/model` Current Behavior

`/connect`:
- `/connect` (no args) → `show_connect_dashboard()` lists all 25 providers with status
- `/connect <provider>` → `guide_to_inline_connect()` points to inline form
- `/connect <provider> <key>` → `apply_connect()` writes 0600 store + `providers.toml` + switches engine

`/model`:
- `/model` (no args) → opens `ModelPickerWidget` (provider tabs → model list)
- `/model anthropic/claude-sonnet-4-20250514` → full ref
- `/model anthropic/sonnet` → provider-prefix alias
- `/model sonnet` → bare alias (resolves via current provider)
- Tab completion: `["opus", "sonnet", "haiku"]` only — `fast/mini/standard/pro/auto` not exposed

**Critical gap**: `/model` currently writes only to `repl::preferences` (`~/.shannon/preferences.json`), not to `~/.shannon/providers.toml`. This means the `connected` profile layer does not pick up `/model` switches. ADR-0005 Phase 3 marked this 🔄 Partial; the `--save` option is unimplemented.

---

## 3. Design Decisions (User-Approved)

The user confirmed four architectural choices:

| Question | Decision |
|---|---|
| How should `/help` render? | **Independent overlay** (reuse onboarding overlay pattern). Do not pollute LLM context. |
| Model-tier strategy? | **Expose explicit tier + config surface**. Do not auto-route via `ModelRouter`. |
| Status Card position? | **Top of Chat welcome area** (above Try-asking examples). |
| Tier naming? | **Canonical: `fast` / `standard` / `pro`**. Aliases accepted as input: Anthropic's `haiku` / `sonnet` / `opus`; others (`flash` / `mini` / `plus` / `ultra` / `max`). Persisted state and toml use canonical names only. |

---

## 4. Target Architecture

### 4.1 First-Screen Status Card

Insert a new `StatusCardWidget` into the chat-welcome area, above the existing Try-asking examples (`widgets/chat.rs:680-767`). The card renders:

```
Active: [anthropic] [claude-sonnet-4-20250514]  Tier: [Standard]
Available providers (4 connected / 25 supported):
  ● anthropic  claude-opus-4 · claude-sonnet-4 · claude-haiku-4-5
  ● openai     gpt-4o · gpt-4o-mini · o1-preview
  ○ ollama    [local: 5 models]
  ○ zhipu     glm-4-plus · glm-4-flash
  ...
Commands: /connect · /model · /provider · /profile · /help
```

When no provider is connected:

```
⚠ No provider connected. Run /connect to get started.
Available providers (0 connected / 25 supported):
  ○ anthropic  claude-opus-4 · ...
  ...
Commands: /connect · /help
```

**Narrow-terminal handling**: below 80 columns, collapse the multi-line block into a single pill (similar to the existing Header collapse logic at `widgets/mod.rs:201-232` using `COLLAPSE_HEADER_WIDTH=60`).

**Data sources**:
- `state.selected_provider: Option<LlmProvider>` — current provider
- `state.model: Option<String>` — current model id
- `connect_status()` (`shannon-ui/src/repl/commands/config.rs:218`) — provider connection states
- `MODEL_CATALOG` (`shannon-core/src/model_registry.rs:78-589`) — provider→models mapping
- `tier_label()` (new) — tier classification for the active model

### 4.2 StatusBar Upgrade

Modify `StatusBarWidget::render_with_spinner` at `widgets/status_bar.rs:178-198`:

```rust
// Before:
[claude-sonnet-4-20250514]

// After:
[anthropic/sonnet · standard]
```

If no model configured: `[No model connected]` in `theme.warning` color (already exists).

### 4.3 `/help` Overlay (Not Chat Message)

Replace `handle_help` in `repl/commands/mod.rs:554-566`:

```rust
// Before:
fn handle_help(repl: &mut Repl, args: &str) -> Result<()> {
    let help_text = shannon_commands::help_utils::generate_help(Some(args));
    repl.chat.add_message(ChatRole::System, help_text);  // pollutes LLM context
    Ok(())
}

// After:
fn handle_help(repl: &mut Repl, args: &str) -> Result<()> {
    repl.state.help_overlay = Some(HelpOverlayState {
        filter: (!args.is_empty()).then(|| args.to_string()),
        selected_category_idx: 0,
        selected_command_idx: 0,
        search_query: String::new(),
    });
    Ok(())
}
```

**Overlay rendering**: Reuse the modal-overlay pattern from `repl/render.rs:1238-1413` `render_onboarding_overlay`. Add a new `render_help_overlay` call inside `draw_frame` (`repl/render.rs:115`) before the main canvas draw, similar to the onboarding path.

**Interaction**:
- `j/k` or `↓/↑` — switch category
- `Enter` — drill into command detail
- `/` — open search filter
- `Esc` — close overlay (writes `repl.state.help_overlay = None`)
- Overlay does NOT touch `repl.chat.messages` — the entire chat history remains unchanged

**Defensive placeholder rename** (still recommended, even though the overlay approach prevents context pollution, to keep snapshot tests clean and reduce LLM confusion risk if the overlay ever gets refactored):

```rust
// crates/shannon-commands/src/builtin/help.rs (7 places)
.with_arg_hint("<FILE_PATH> <LINE:int> <CHARACTER:int>")        // was "<file> <line> <character>"
.with_arg_hint("<FILE_PATH> <LINE:int> <CHARACTER:int>")        // find_references
.with_arg_hint("<FILE_PATH> <LINE:int> <CHARACTER:int>")        // hover
.with_arg_hint("<FILE_PATH>")                                   // document_symbol
.with_arg_hint("<FILE_PATH> <LINE:int> <CHARACTER:int> <NEW_NAME>")  // rename_symbol
.with_arg_hint("<FILE_PATH> <START_LINE:int> <START_CHAR:int> <END_LINE:int> <END_CHAR:int>")  // code_actions
```

The all-caps convention breaks the visual similarity with HTML/XML tags and reduces the chance of LLM misidentification even in edge cases.

### 4.4 Model Tier Surface

#### 4.4.0 Tier Naming Decision (Approved)

**Canonical tier names**: `fast` / `standard` / `pro` (semantic, provider-agnostic).
**Aliases**: `haiku` / `sonnet` / `opus` (Anthropic-style); `flash` / `mini` / `nano` / `plus` / `ultra` / `max` (other providers); `auto` (reserved).

Rationale (validated against the live `MODEL_CATALOG` of ~50 models):
- Only Google Gemini and Zhipu use "Flash" as a model name; using it as a universal tier name would falsely imply Anthropic has a "Flash" model.
- "Pro" is the industry-standard tier suffix (GitHub Pro, JetBrains Pro, Adobe Pro, Linear Pro, Notion Pro). "Prod" is ambiguous with "production environment" and is rejected.
- Haiku/Sonnet/Opus aliases let Anthropic-trained users express their intent in familiar terms; the resolver normalizes them to Fast/Standard/Pro internally.
- `toml` files, status pills, logs, and metrics use **only** the canonical names; aliases are input-only.

#### 4.4.1 New Type Definitions

In `crates/shannon-types/src/provider_config.rs`:

```rust
/// Tier name as stored in configuration and persisted state.
/// Always serialize/deserialize as the canonical name (`fast`/`standard`/`pro`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TierName {
    Fast,
    Standard,
    Pro,
    Auto,
}

impl TierName {
    /// Canonical lowercase name used in toml, logs, status pills.
    pub fn canonical(self) -> &'static str {
        match self {
            TierName::Fast => "fast",
            TierName::Standard => "standard",
            TierName::Pro => "pro",
            TierName::Auto => "auto",
        }
    }

    /// Human-readable label for UI display (capitalized).
    pub fn display(self) -> &'static str {
        match self {
            TierName::Fast => "Fast",
            TierName::Standard => "Standard",
            TierName::Pro => "Pro",
            TierName::Auto => "Auto",
        }
    }

    /// Normalize any accepted user input to the canonical TierName.
    /// Accepts both canonical names and provider-native aliases.
    /// Case-insensitive.
    pub fn from_user_input(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            // Canonical (preferred; written to toml + logs)
            "fast"     => Some(TierName::Fast),
            "standard" => Some(TierName::Standard),
            "pro"      => Some(TierName::Pro),
            "auto"     => Some(TierName::Auto),
            // Aliases — provider-native names that imply the same tier
            "flash" | "mini"  | "nano" | "haiku"   => Some(TierName::Fast),
            "plus"  | "sonnet"| "medium"| "turbo"  => Some(TierName::Standard),
            "opus"  | "ultra" | "max"   | "large"  => Some(TierName::Pro),
            _ => None,
        }
    }

    /// Tab-completion suggestions shown to the user (canonical first, aliases after).
    pub fn suggestions() -> &'static [&'static str] {
        &[
            "fast", "standard", "pro", "auto",  // canonical
            "haiku", "sonnet", "opus",          // Anthropic
            "flash", "mini", "plus", "ultra",   // others
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProviderTiers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pro: Option<String>,
}

// Modify ProviderProfile:
pub struct ProviderProfile {
    // ... existing fields ...
    #[serde(default)]
    pub tiers: ProviderTiers,
}
```

In `crates/shannon-core/src/model_registry.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierLabel { Fast, Standard, Pro, Unknown }

impl ModelInfo {
    pub fn tier_label(&self) -> TierLabel {
        if self.capabilities.contains(ModelCapabilities::CHEAP)
            || self.capabilities.contains(ModelCapabilities::SPEED) {
            TierLabel::Fast
        } else if self.id.contains("opus")
            || self.id.contains("o1")
            || self.id.contains("ultra")
            || self.id.contains("max") {
            TierLabel::Pro
        } else {
            TierLabel::Standard
        }
    }
}

/// Resolve a tier (canonical or alias) to a concrete model id for a provider.
/// Order of preference:
///   1. User-configured `profile_tiers.<canonical>` override
///   2. Catalog match using `ModelCapabilities` (Fast ⇒ CHEAP|SPEED, Pro ⇒ REASONING)
///   3. Internal `ModelTier` enum fallback (Opus/Sonnet/Haiku)
///   4. None (caller should display "tier not available for this provider")
pub fn resolve_tier(
    tier_input: &str,                  // accepts "fast" / "flash" / "haiku" alike
    provider: &LlmProvider,
    profile_tiers: &ProviderTiers,
) -> Option<String> {
    let tier = TierName::from_user_input(tier_input)?;
    if matches!(tier, TierName::Auto) { return None; }   // reserved

    // 1. Explicit user override in providers.toml
    let explicit = match tier {
        TierName::Fast => &profile_tiers.fast,
        TierName::Standard => &profile_tiers.standard,
        TierName::Pro => &profile_tiers.pro,
        TierName::Auto => return None,
    };
    if let Some(id) = explicit { return Some(id.clone()); }

    // 2. Catalog-based inference (preferred; uses ModelCapabilities)
    let wanted = match tier {
        TierName::Fast => ModelCapabilities::SPEED | ModelCapabilities::CHEAP,
        TierName::Standard => ModelCapabilities::CODING,
        TierName::Pro => ModelCapabilities::REASONING,
        TierName::Auto => return None,
    };
    if let Some(m) = catalog_models_for_provider(provider)
        .iter()
        .filter(|m| m.capabilities.contains(wanted))
        .min_by_key(|m| m.cost_tier)   // Fast ⇒ cheapest; Pro ⇒ strongest
    {
        return Some(m.id.to_string());
    }

    // 3. Internal ModelTier fallback (legacy path; still works)
    let mt = match tier {
        TierName::Fast => ModelTier::Haiku,
        TierName::Standard => ModelTier::Sonnet,
        TierName::Pro => ModelTier::Opus,
        TierName::Auto => return None,
    };
    resolve_model_alias(mt.into_alias(), Some(provider)).map(|s| s.to_string())
}
```

#### 4.4.2 `/model --tier` Command Surface

Extend `handle_model` (`crates/shannon-ui/src/repl/commands/config.rs:35-85`):

**Argument syntax** (fixed order, accepts canonical names OR aliases):
- `/model --tier fast` — switch current provider to its fast tier (canonical)
- `/model --tier flash` — same as above (alias, normalized internally)
- `/model --tier haiku` — same as above (Anthropic alias)
- `/model --tier fast anthropic` — switch provider `anthropic` to its fast tier
- `/model --tier pro --save` — switch + persist to `~/.shannon/providers.toml`
- `/model --tier auto` — reserved (returns error "auto routing not yet supported")

```rust
// New args parsing (before existing logic):
if args.starts_with("--tier") {
    // tokens: ["--tier", "<tier>", "[provider]", "[--save]"]
    let parts: Vec<&str> = args.split_whitespace().collect();
    let tier_input = parts.get(1).copied().unwrap_or("");
    let tier = TierName::from_user_input(tier_input)
        .ok_or_else(|| anyhow!(
            "Unknown tier '{}'. Try: {}",
            tier_input,
            TierName::suggestions().join(", ")
        ))?;
    if matches!(tier, TierName::Auto) {
        return Err(anyhow!("--tier auto is reserved for a future spec"));
    }
    let explicit_provider = parts.get(2).copied();
    let save = parts.iter().any(|p| *p == "--save");
    let provider = explicit_provider
        .map(parse_provider_name)
        .transpose()?
        .or(repl.state.selected_provider)
        .ok_or_else(|| anyhow!("No provider selected; specify one: /model --tier <tier> <provider>"))?;
    let profile_tiers = load_provider_tiers(&provider);
    let model_id = resolve_tier(tier_input, &provider, &profile_tiers)
        .ok_or_else(|| anyhow!("No model found for tier={} provider={}", tier.canonical(), provider))?;
    repl.state.model = Some(model_id.clone());
    repl.state.selected_provider = Some(provider.clone());
    engine.set_model_for_provider(&model_id, &provider);
    save_preferences(...);
    if save {
        persist_model_to_providers_toml(&provider, &model_id, tier)?;
    }
    return Ok(());
}
```

**Tab completion** (canonical first, then aliases):
```rust
// model_registry.rs:859 (existing — extend)
pub fn model_aliases() -> &'static [&'static str] {
    // From ["opus", "sonnet", "haiku"] to:
    TierName::suggestions()  // canonical + aliases
}
```

#### 4.4.3 Picker Tier Tabs

Modify `ModelPickerWidget` (`crates/shannon-ui/src/widgets/select.rs:787-960`) to add tier-tab navigation:

```
[anthropic] [openai] [ollama] [zhipu] ...      ← provider tabs (existing)
   |
   └── [Fast] [Standard] [Pro] [Auto]          ← tier tabs (NEW)
         |
         └── [haiku-4-5] [haiku-3-5] [haiku-3]  ← models in tier
```

Tab-cycling: `Tab` / `Shift+Tab` for tier, `←` / `→` for provider.

#### 4.4.4 Persistence — `/model --save`

Currently `/model` writes only to `repl::preferences` (`~/.shannon/preferences.json`). The new `--save` flag writes the model switch to `~/.shannon/providers.toml` so the change survives across sessions and is visible to the `connected` profile layer (ADR-0005 Phase 3 🔄 Partial → ✅).

```rust
fn persist_model_to_providers_toml(provider: &LlmProvider, model_id: &str) -> Result<()> {
    let mut store = ProviderConfigStore::load_or_default();
    let profile = store.ensure_provider(provider);
    profile.tiers.standard = Some(model_id.to_string());  // or whichever tier it falls into
    store.save()?;  // 0600 atomic
    Ok(())
}
```

---

## 5. Component Boundaries

| Component | Responsibility | Inputs | Outputs |
|---|---|---|---|
| `StatusCardWidget` (NEW) | Render first-screen status: provider/model/tier + available providers/models | `ReplState`, `MODEL_CATALOG`, `connect_status()` | ratatui draw |
| `HelpOverlay` (NEW) | Render /help as modal overlay | `help_utils::generate_help(filter)`, keyboard events | ratatui draw + state mutation |
| `StatusBarWidget` (EXTEND) | Add provider + tier to model pill | `ReplState` | ratatui draw |
| `ModelPickerWidget` (EXTEND) | Add tier tab layer between provider and model list | `MODEL_CATALOG`, `ReplState` | ratatui draw + state mutation |
| `TierResolver` (NEW, in `model_registry.rs`) | Map `&str` (canonical or alias) × `LlmProvider` × `ProviderTiers` → model id | `&str`, `LlmProvider`, `ProviderTiers` | `Option<String>` |
| `TierName::from_user_input` (NEW) | Normalize any accepted input (canonical + aliases) to canonical TierName; case-insensitive | `&str` | `Option<TierName>` |
| `handle_help` (MODIFY) | Open/close `HelpOverlay` instead of writing chat message | `args: &str` | `ReplState.help_overlay` |
| `handle_model` (EXTEND) | Add `--tier` and `--save` parsing | `args: &str` | `ReplState.model/selected_provider`, `providers.toml` |

---

## 6. State Machine Additions

Add to `ReplState`:

```rust
// crates/shannon-ui/src/repl/state.rs
pub struct HelpOverlayState {
    pub filter: Option<String>,
    pub selected_category_idx: usize,
    pub selected_command_idx: usize,
    pub search_query: String,
}

pub struct ReplState {
    // ... existing fields ...
    pub help_overlay: Option<HelpOverlayState>,
}
```

Modal-overlay state machine (reuse onboarding pattern):

```
Closed ──[handle_help called]──> Open(filter)
Open ──[Esc pressed]──> Closed
Open ──[/ search opened]──> SearchActive
SearchActive ──[Enter]──> Open (with filter applied)
```

---

## 7. Data Flow

### 7.1 Startup → First Frame

```
CLI launch
  ↓
ConfigBuilder.build() → ShannonConfig
  ↓
LlmClientConfig::from(merged)
  ↓
Repl::new() ← Preferences.load()
  ↓
ReplState { model, selected_provider, help_overlay: None, ... }
  ↓
draw_frame()
  ├─ StatusCardWidget::render(area, &state)         ← NEW
  ├─ ChatWidget::render(area, &state)               ← existing, now with Try-asking below card
  ├─ PromptWidget::render(area, &state)            ← existing
  ├─ StatusBarWidget::render(area, &state)         ← extended with provider/tier
  └─ render_help_overlay(&state) if Some           ← NEW
```

### 7.2 `/model --tier fast --save`

```
User types: /model --tier fast --save
  ↓
repl/commands/mod.rs::dispatch → handle_model
  ↓
parse_tier_flag(args) → TierFlag { tier: Fast, save: true }
  ↓
resolve_tier(Fast, current_provider, profile_tiers)
  ↓
"claude-haiku-4-5" (or profile override)
  ↓
repl.state.model = Some("claude-haiku-4-5")
engine.set_model_for_provider()
Preferences.save()
  --save → persist_model_to_providers_toml()  ← NEW
```

---

## 8. Testing Strategy

| Test | Type | Key Assertions |
|---|---|---|
| `StatusCard::render` empty state | widget test | Shows "⚠ No provider connected" banner |
| `StatusCard::render` configured | widget test | Shows provider + model + tier label correctly |
| `StatusCard::render` narrow terminal (< 80 cols) | widget test | Collapses to single pill |
| `HelpOverlay::open/close` | widget test | Opens on `/help`, closes on `Esc` |
| `handle_help` does NOT mutate chat | integration test | `repl.chat.messages.len()` unchanged after `/help` |
| `arg_hint` placeholder rename | snapshot test | No `<file>`, `<line>`, `<character>` substrings in help output |
| `TierResolver::resolve_tier` | unit | anthropic + Fast → "claude-haiku-4-5"; profile override wins |
| `TierResolver::resolve_tier` aliases | unit | "flash"/"haiku"/"mini" all resolve to claude-haiku-4-5; "opus"/"ultra" to claude-opus-4 |
| `TierName::from_user_input` | unit | "FAST" (uppercase) → Fast; "Haiku" (mixed case) → Fast; "garbage" → None |
| `TierName::canonical` round-trip | unit | `TierName::from_user_input(t.canonical()) == Some(t)` for all 4 variants |
| `TierName::suggestions` first | unit | First 4 entries are exactly `["fast", "standard", "pro", "auto"]` |
| `ModelInfo::tier_label` | unit | opus → Pro; haiku → Fast; sonnet → Standard |
| `handle_model --tier fast` | integration | `repl.state.model = Some("claude-haiku-4-5")` |
| `handle_model --tier haiku` | integration | alias → same result as `--tier fast` |
| `handle_model --tier fast anthropic` | integration | switches provider + tier atomically |
| `handle_model --tier pro --save` | integration | `providers.toml` tier field uses canonical `"pro"` (not `"opus"`) |
| `handle_model --tier auto` | integration | returns error "auto routing not yet supported" |
| `handle_model --tier xyz` | integration | returns error with suggestion list |
| `ProviderProfile.tiers` round-trip | schema test | JsonSchema validation passes |
| `model_aliases()` Tab completion | unit | Canonical first 4 + Anthropic aliases (`haiku`/`sonnet`/`opus`) present |
| `persist_model_to_providers_toml` writes canonical name | snapshot | toml content uses `fast`/`standard`/`pro` (not aliases) |

---

## 9. Implementation Milestones

| # | Title | Effort | Priority | Files Touched |
|---|---|---|---|---|
| M1 | `/help` overlay + arg_hint rename | 1-2 days | 🔴 P0 | `repl/commands/mod.rs`, `repl/render.rs`, `repl/state.rs`, `builtin/help.rs` (7 lines) |
| M2 | First-screen Status Card + StatusBar upgrade | 2-3 days | 🟡 P1 | `widgets/status_card.rs` (new), `widgets/chat.rs`, `widgets/status_bar.rs`, `widgets/mod.rs`, `repl/render.rs` |
| M3 | Model Tier types + `/model --tier --save` + Picker tier tabs + alias normalization (canonical: fast/standard/pro; aliases: haiku/sonnet/opus/flash/mini/plus/ultra/max) | 3-4 days | 🟡 P1 | `provider_config.rs`, `model_registry.rs`, `commands/config.rs`, `widgets/select.rs` |
| M4 | Docs, CHANGELOG, ADR-0005 update, i18n | 1 day | 🟢 P2 | `docs/adr/0005-*.md`, `CLAUDE.md`, `CHANGELOG.md`, `locales/*.yaml` |

**Total estimated effort**: 7-10 working days (single engineer).

---

## 10. Risks & Mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| HelpOverlay collides with onboarding overlay state | 🟡 Medium | Unify into a single `ReplState.modal: Option<Modal>` enum (`Modal::Onboarding | Modal::Help`) |
| StatusCard overwhelms narrow terminals | 🟡 Medium | Collapse to single pill below 80 cols (mirror `COLLAPSE_HEADER_WIDTH=60`) |
| `--tier` flag conflicts with `provider/model` syntax | 🟢 Low | Explicit `--tier` prefix; bare `/model sonnet` keeps working unchanged |
| `ProviderTiers` toml schema breaks old configs | 🟢 Low | `#[serde(default)]` on the new field; old files load with empty tiers |
| Arg-hint rename breaks snapshot tests | 🟡 Medium | Update snapshots in lockstep; label the migration in CHANGELOG |
| Auto tier routing breaks user expectations | 🔴 High | **Do not enable** `ModelRouter::recommend()` in production — explicit tier only |
| `persist_model_to_providers_toml` accidentally leaks API key | 🟢 Low | Function only writes model id, never credential; `providers.toml` is config-only per ADR-0005 D4 |

---

## 11. Out of Scope (Deferred)

These are intentionally **not** in this spec:

- **ModelRouter auto-routing by TaskType**: `ModelRouter::recommend()` exists but has zero callers. Connecting it to `QueryEngine` requires product decisions about user expectations — defer to a separate spec.
- **`AuxRole` consumption**: `ModelProfile.auxiliary` HashMap is reserved for assigning different models to Vision/Compression/TitleGeneration tasks. Requires per-task routing logic — defer.
- **`fallback_models` consumption**: `ProviderProfile.fallback_models` is reserved for fallback on provider errors. Requires retry policy decisions — defer.
- **Dynamic model catalog via models.dev**: ADR-0005 Phase 5 (deferred).
- **Tauri desktop first-screen mirroring**: The desktop app's first-screen should match, but the desktop REPL integration is its own workstream.

---

## 12. Success Criteria

After this design is fully implemented:

1. ✅ First screen shows current provider + model + tier + available providers/models list + 5 command hints
2. ✅ `/help` (with or without args) opens an overlay; chat history length is unchanged; LLM never sees `<file>` literals
3. ✅ `/model --tier fast` switches to the cheapest/fastest model for the current provider
4. ✅ `/model --tier haiku` (Anthropic alias) and `/model --tier flash` (Gemini alias) both switch to the same fast-tier model
5. ✅ `/model --tier pro --save` persists the **canonical** tier name (`pro`) — never the alias — to `~/.shannon/providers.toml`
6. ✅ ModelPickerWidget has provider → tier → model three-level navigation
7. ✅ Tab completion shows canonical names first (`fast`/`standard`/`pro`/`auto`) followed by Anthropic aliases (`haiku`/`sonnet`/`opus`)
8. ✅ All existing tests pass; new tests added for each milestone
9. ✅ ADR-0005 Phase 3 marked ✅ (instead of 🔄 Partial); Phase 4 partially ✅

---

## 13. Open Questions

None at draft time. The user approved all three architectural choices during design review:

- `/help` → independent overlay
- Tier strategy → explicit tier + config surface (no auto routing)
- Status Card position → top of Chat welcome area