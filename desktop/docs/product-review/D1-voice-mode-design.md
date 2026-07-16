# D1 — Voice Mode Design

**Status:** Planning / not yet implemented
**Owner:** ed
**Last updated:** 2026-06-26
**Estimated effort:** 2–3 days (UI), +3–5 days if backend voice pipeline is needed

## Why

Voice mode is the single biggest consumer-grade gap versus ChatGPT
Desktop, which ships it as the defining feature of its desktop app.
Claude has voice on mobile. Hermes has voice in CLI + desktop. Shannon
has no voice surface at all today (`05c` competitive analysis).

The product positioning decision (2026-06-22) is that Shannon Desktop
targets general users while remaining compatible with developers. Voice
mode is the highest-leverage feature to close that gap — it transforms
the product from "dev tool with a chat box" into "ambient assistant
you can talk to while doing other things."

## Scope (in / out)

**In scope (MVP):**
- Push-to-talk and tap-to-talk input from the chat composer
- Live transcription display (user sees their words as they speak)
- Voice output for assistant responses (TTS) with a mute toggle
- Animated "orb" visualization that pulses while listening / speaking
- Permission gate: microphone access requested on first use

**Out of scope (MVP):**
- Wake-word ("Hey Shannon") — requires always-on mic + on-device VAD
- Full duplex barge-in (user interrupts assistant mid-speech)
- Voice-only mode (no keyboard fallback) — keep text as primary input
- Multi-speaker / meeting transcription
- Custom voice cloning

## UX design

### Composer integration

A microphone icon button is added to the right side of the chat
composer's action row, between the existing attach-file and send
buttons. States:

| State | Visual | Behavior |
|-------|--------|----------|
| Idle (mic available) | Outline mic icon, default color | Click → start recording |
| Idle (mic denied) | Mic-off icon, grayed | Click → toast: "Microphone access denied. Enable in Settings." |
| Recording | Filled mic icon, pulsing red ring | Click → stop + transcribe |
| Transcribing | Spinner inside mic button | Disabled, shows "Transcribing..." tooltip |
| Playing back | Outline mic icon + equalizer anim | Click → interrupt TTS |

### Orb visualization

A 64px orb rendered with CSS (no canvas dependency). Three modes:

- **Idle:** subtle breathing animation, primary color at 30% opacity
- **Listening:** faster pulse, red-tinted, scales 1.0 ↔ 1.1
- **Speaking:** waveform-like ripple, primary color at full opacity

The orb appears above the chat composer when voice mode is active
(recording or playing back). When idle, it collapses back into the
mic button.

### Settings panel additions (Settings → Voice)

- Voice selection dropdown (provider-dependent voices)
- Speaking speed slider (0.75× — 2×)
- Auto-speak responses toggle (default off — users opt in)
- Push-to-talk shortcut configuration (default: hold Space)

## Technical design

### Provider matrix

Voice needs two halves: Speech-to-Text (STT) for input and Text-to-Speech
(TTS) for output. Different providers offer different combinations.

| Provider | STT | TTS | Notes |
|----------|-----|-----|-------|
| OpenAI | Whisper API + Realtime API | OpenAI Voices (11 voices) | Realtime API gives lowest latency (≤300ms) but needs WebSocket |
| Anthropic | No native STT | No native TTS | Anthropic voice is mobile-app-only as of 2026-Q2 — no public API |
| Google | Gemini Live API | Gemini TTS | Strong STT quality; TTS more robotic than OpenAI |
| Local | whisper.cpp + piper | piper | Zero per-call cost; initial setup heavier; quality acceptable |

**Recommendation:** Phase 1 ships with OpenAI (best quality, easiest
API). Provider switching is added in Phase 2 once the abstraction is
proven.

### New Tauri commands (Rust side)

```rust
// src/commands_voice.rs (new module)
#[tauri::command]
async fn start_recording(app: AppHandle, provider: String) -> Result<()>

#[tauri::command]
async fn stop_recording() -> Result<Transcript>

#[tauri::command]
async fn speak_text(app: AppHandle, text: String, voice: String) -> Result<()>

#[tauri::command]
async fn stop_speaking() -> Result<()>

#[tauri::command]
async fn list_voices(provider: String) -> Result<Vec<Voice>>
```

Audio capture uses `cpal` (already in workspace via `shannon-core`).
Encoding to Opus via `opus-rs`. Network send to provider via existing
`reqwest` stack.

### Frontend layer

```
ui/src/components/voice/
├── VoiceOrb.tsx          # animated visualization
├── MicButton.tsx         # composer-integrated button
├── VoiceSettings.tsx     # settings panel section
└── useVoice.ts           # hook: startRecording / stopRecording / speak / stopSpeaking
```

The hook subscribes to Tauri events:
- `voice://transcript-partial` — partial STT result, displayed live
- `voice://transcript-final` — final STT result, appended to composer
- `voice://tts-start` / `voice://tts-end` — orb state sync
- `voice://error` — surface as toast

### Permissions

- Tauri `microphone` plugin (already in `tauri-plugin-permission`)
- First use: OS-level permission dialog
- Denial path: read `status()` and disable mic button with tooltip

## Phased plan

### Phase 1 — STT input only (1.5 days)

- Backend: `start_recording` / `stop_recording` with OpenAI Whisper
- Frontend: MicButton in composer, transcript appends to input
- No orb yet — just button state changes
- Settings: API key + provider selector only

Exit: User can dictate a message, text appears in composer, send as
normal. This alone closes 60% of the consumer-perceived gap.

### Phase 2 — TTS output (1 day)

- Backend: `speak_text` with OpenAI Voices
- Frontend: Orb appears during playback, MicButton interrupts TTS
- Settings: voice selection, speed slider, auto-speak toggle

Exit: Assistant responses are audible. Combined with Phase 1 this is
the full ChatGPT-Desktop-equivalent voice loop.

### Phase 3 — Provider switching (1 day)

- Add Google Gemini Live + local whisper.cpp providers
- Settings: provider dropdown with quality/cost hints
- Backend: provider-agnostic trait + per-provider impl

Exit: User can choose based on privacy / cost / quality preference.

## Risks

- **Microphone permission UX varies by OS.** macOS is strict; Linux
  PipeWire setup may need user intervention. Plan for a setup wizard.
- **Latency perception.** Anything >500ms total round-trip feels broken.
  Mitigation: stream partial transcripts; start TTS before the full
  response is finished.
- **OpenAI Realtime API is in beta.** Pricing and protocol may shift.
  Keep the WebSocket layer isolated behind a trait.

## Acceptance criteria

- [ ] User can hold the mic button, speak, see live transcript, release
      to commit text to the composer
- [ ] User can click play on any assistant message to hear it spoken
- [ ] Orb animation is smooth at 60fps in Chrome and WebKit (Tauri)
- [ ] Mic permission denial is handled gracefully with a clear path
      to re-enable
- [ ] Voice settings persist across sessions
- [ ] Voice mode works with the keyboard still — no mode lock-in

## Dependencies

- Tauri plugin: `microphone` (add to `Cargo.toml`)
- Rust crate: `cpal`, `opus-rs`, `reqwest` (already in workspace)
- Engine: none (voice is a UI-shell feature, doesn't touch reasoning)
