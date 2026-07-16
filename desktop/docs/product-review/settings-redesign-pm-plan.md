# Settings Redesign — PM Improvement Plan (Models + Notifications)

**Author:** PM review · **Date:** 2026-06-27 · **Status:** Proposal, awaiting approval

A senior-PM review of two Settings areas the user flagged as "配置方式和 UI
组件设计不合理" (configuration flow + UI components feel unreasonable):
**(1) 配置 → 模型配置 (Models)** and **(2) 设置 → 通知 (Notifications)**.
Grounded in competitor research + a read of the current Shannon code
(`ui/src/components/settings/ModelsSettings.tsx`,
`NotificationsSettings.tsx`).

---

## 0. Methodology & an honest caveat on the named competitors

The brief named reasonix, hermes desktop, codex desktop, claude desktop.
**Honest caveat:** Claude Desktop and Codex/OpenAI are **single-vendor** —
they have no multi-vendor configuration UI at all, so they are not useful
benchmarks for *multi-provider* configuration. "reasonix" / "hermes desktop"
have no authoritative public documentation I could verify, so I will not
fabricate findings about them.

The **relevant** benchmarks for multi-vendor config are the open-source
multi-provider clients that actually solve this problem every day:
**LibreChat, Cherry Studio (47.5k★), LobeChat, AnythingLLM, Jan, Chatbox,
Msty, LM Studio.** For notifications: **Linear, Slack** (which rebuilt its
notification system in 2026), plus general notification-UX best practice
(Adobe/Setproduct). All findings below are sourced from these.

---

## 1. Models Configuration (`ModelsSettings.tsx`)

### 1.1 Current state — what's there today
The page stacks six sections top-to-bottom:
1. **Performance Strategy** — segmented control (balanced / speed / high-quality).
2. **Active Model** card — shows the one current model + provider.
3. **Quick Setup Presets** — 7 hardcoded provider cards (OpenAI, Anthropic,
   DeepSeek, GLM, MiniMax, Kimi, Ollama); each expands to an inline API-key
   field → `switchProvider`.
4. **Provider Tabs + Available Models** — model list, click to switch.
5. **API Key** — a *second*, single global key input (test / save) labeled
   "API Connection — {provider}".
6. **Global Parameters** — temperature + max-tokens sliders.

### 1.2 Problems (root causes, not symptoms)

| # | Problem | Evidence |
|---|---------|----------|
| M1 | **Two conflicting API-key paths.** Presets (§3) and the global key box (§5) both set a key but look unrelated; users can't tell which is authoritative. | `submit()` → `switchProvider({api_key})` (L303) vs `handleSaveKey()` → `configure({key:'api_key'})` (L44) |
| M2 | **Single active provider.** The whole page assumes one provider at a time (`status.provider`, `switchProvider` replaces it). You can't keep several providers configured and hop between them. | `handleModelSwitch` (L28) |
| M3 | **Hardcoded 7 presets, no custom/OpenAI-compatible provider.** `PROVIDER_PRESETS` is a baked-in array; base URL is preset-locked and hidden. | L269-277 |
| M4 | **Sliders show fake values.** `value={0.7}` / `value={4096}` are literals, not read from config — they reset on every remount and never reflect reality. | L250-251 |
| M5 | **"Settings maze" risk** — Cherry Studio's own reviewers flag this as the #1 failure mode of multi-provider apps; Shannon is trending the same way (6 stacked sections, overlapping concepts). | §1.1 |
| M6 | **No connection health per provider** — can't see at a glance which providers are valid/connected (only an "active" badge). | — |

### 1.3 What the best competitors do
- **LibreChat** — providers are "**Endpoints**": a managed list where you
  *add* an endpoint (name, API key, base URL, models), each independently
  **testable** and toggleable; you switch active endpoint from a picker.
  Multi-provider is the core feature, not a side panel.
- **Cherry Studio / LobeChat** — a dedicated **"Providers"** section: each
  provider is a row/card with its own key + base URL + model list + a
  **"Test" / connection-status** indicator. Critically, the reviewer's
  advice: *"choose one provider first… do not start by connecting every
  provider — that creates confusion."*
- **Jan / Chatbox / Msty** — provider = a saved connection you can name,
  edit, duplicate, delete; base URL editable so any OpenAI-compatible
  endpoint works (local, proxy, gateway).

### 1.4 Proposed redesign — "Providers as first-class connections"

**Guiding principle:** model the user's mental model — *"I have several AI
accounts; let me connect each once, see which work, and pick the one I want
right now."* Collapse the 6 sections into 3.

**§A — Providers (managed list, replaces Presets + the global API-Key box)**
- A list of **saved provider connections** (not presets). Each row:
  provider name/logo · status dot (● connected / ○ invalid / + not set) ·
  base URL (editable, so OpenAI-compatible gateways/proxies/Ollama all fit)
  · **Test** button · edit / delete.
- **Add provider** = pick a template (OpenAI, Anthropic, DeepSeek, GLM,
  MiniMax, Kimi, Ollama, **or Custom OpenAI-compatible**) then fill key +
  base URL + default model. Several can be connected at once (fixes M2/M3).
- This single section is the **only** place API keys live (fixes M1).

**§B — Active model (picker)**
- A compact "current model" control: provider dropdown (populated from
  connected providers) → model dropdown (that provider's models). One
  obvious place to switch (fixes the Active-Model-card vs Provider-Tabs
  redundancy).

**§C — Generation defaults**
- Performance strategy (keep) + temperature/max-tokens sliders that
  **actually read & write config** (fixes M4). Group as "defaults"; per-model
  overrides can come later.

**Phasing:** Phase 1 = fix M1 + M4 (merge the two key paths; wire sliders to
config) — small, high-trust. Phase 2 = M2/M3/M6 (Providers-as-connections
list + custom endpoint + per-provider health). Phase 3 = M5 polish.

---

## 2. Notifications (`NotificationsSettings.tsx`)

### 2.1 Current state
Three stacked blocks:
1. **WebhookSection** — one outbound webhook; a preset selector
   (feishu/dingtalk/wechat/slack/custom) + URL + save + a "danger zone" clear.
2. **OutboundSection** — a *second* outbound block (separate component).
3. **Inbound** — 3 channel cards (Slack / Telegram / Email); click → wizard.
   Email's wizard only toasts "coming soon" (backend not implemented).

### 2.2 Problems

| # | Problem | Evidence |
|---|---------|----------|
| N1 | **Two fragmented outbound blocks.** WebhookSection + OutboundSection both feel like "where I configure outgoing notifications"; relationship is unexplained. | L380, L383 |
| N2 | **Single webhook only.** Can't fan out to multiple destinations. | WebhookSection |
| N3 | **No "what triggers a notification" control.** There's no UI for choosing *which events* fire (task complete, permission request, error, agent message). The webhook just fires on whatever the backend decides. Linear/Slack both expose per-event-type toggles. | — |
| N4 | **No global enable / quiet hours / DND.** Standard in Slack/Linear/OS — missing here. | — |
| N5 | **Dead Email control.** Opens a wizard that toasts "coming soon." A control that does nothing erodes trust. | L454-462 |
| N6 | **Implementation-level vocabulary.** "Webhook template / inbound listener / bot token" exposed to end users; they expect "notify me when X via Y." | §2.1 |
| N7 | **Pull-based status** (30s poll) + no delivery-failure feedback / no test-send in the main flow. | L304-311 |

### 2.3 What the best competitors do
- **Linear** — Settings → Notifications, **organized by channel**
  (Desktop / Mobile / Email / Slack). Each channel has an enabled dot
  (green/gray) and, inside it, a list of **notification types** you toggle.
  Timing differs by channel (push = real-time, email = urgency-based digest).
- **Slack (2026 rebuild)** — **hierarchical preference model**: sane defaults
  (most users keep the default = mentions/DMs), with granular per-channel
  overrides available but not forced. Key lesson: **defaults carry the
  experience; granularity is an escape hatch, not the front door.**
- **Best practice (Adobe/Setproduct):** "give people control over *what* they
  receive and *when*; granular preferences cut irrelevant interruptions — the
  single biggest lever on opt-in rates."

### 2.4 Proposed redesign — "Events × Channels" matrix

**Guiding principle:** let the user say *"notify me about **these events**
through **these channels**."* Invert the current implementation-first layout
into a goal-first one.

**§A — Notification events (what triggers a notification)**
- A checklist of **event types** (task completed, permission/request needs
  review, error/failure, agent message, routine triggered). Each toggleable
  (fixes N3). These become the rows of the matrix.

**§B — Channels (where it goes)**
- Unified list of **destinations**: Desktop (native OS), plus each connected
  outbound channel (Slack, Telegram, Email, webhook groups). Each shows a
  connection dot + a Test/Send-sample button (fixes N7).
- **One** outbound config surface (merge WebhookSection + OutboundSection,
  fixes N1); support multiple webhooks/destinations (fixes N2).

**§C — Event × Channel matrix (the core)**
- Rows = events (§A), columns = channels (§B), cells = on/off. This is
  Linear's "inside a channel, pick types" pattern flipped into a scannable
  grid — the most legible way to express "I want task-completions on Desktop
  only, but errors on Slack too."

**§D — Global controls**
- Master enable + **quiet hours / DND** (fixes N4). Email channel either
  shipped or **disabled with a visible "coming soon"** badge — never a live
  control that no-ops (fixes N5).

**Phasing:** Phase 1 = N1 + N5 (merge outbound; disable-or-ship Email) —
quick trust wins. Phase 2 = N3 + N4 (events list + matrix + DND). Phase 3 =
N2 + N7 (multi-destination, test-send, push-based status).

---

## 3. Cross-cutting notes
- Both redesigns are **frontend-mostly** at Phase 1 (the engine already
  exposes `switch_provider`, `configure`, webhook/inbound APIs). Phase 2/3
  may need light backend support (multiple saved providers, event-type
  routing) — flag for `shannon-code` if pursued.
- **i18n:** both pages are already well-internationalized; the strategy-label
  gap in Models (`Balanced/Speed/High quality` via string ops) and a handful
  of hardcoded `aria-label`s are fixed in the same batch (see CHANGELOG).
- **Scope discipline:** these are *plans*, not commitments. Recommend
  shipping Phase 1 of each first and validating before the larger Phase 2.
