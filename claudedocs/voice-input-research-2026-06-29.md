# Shannon Desktop — Voice Input: Competitive Research & Recommended Plan

**Date:** 2026-06-29 · **Author:** PM/architect lens · **Status:** analysis for decision (D4 follow-up)
**Trigger:** current `useVoice` is a Web Speech API stub that injects `"This is a stub transcript…"` on Linux/macOS. User steer: **API-based STT primary; local-model STT as a later opt-in.**

---

## 1. How competitors actually do it

| Product | STT mechanism | Notes |
|---|---|---|
| **ChatGPT desktop/web** | Cloud **Whisper** (in-house) for dictation; GPT-4o realtime for Voice Mode | Mic → Whisper API → text into prompt. Voice Mode = bidirectional (STT+TTS). |
| **Claude.ai / Claude Code** | `/voice` push-to-talk dictation; Claude Voice Mode = two-way | Streams transcript into the prompt buffer. Cloud STT backend. |
| **Cursor** | No first-party voice — community bolts on **Superwhisper** | Defer to OS-level tooling. |
| **Superwhisper / Willow / turbo-whisper** | Dedicated dictation: record → **Whisper** (cloud OpenAI/Groq **or local whisper.cpp**) → optional LLM polish → paste at cursor | The de-facto "power user" pattern. Supports offline models. |
| **OS dictation** (macOS/Windows) | Built-in system recognition | Free but mediocre accuracy, inconsistent cross-platform, weak zh-CN. |

**Takeaway:** *Every serious AI-chat competitor uses **Whisper-family models** — either a cloud Whisper API or local whisper.cpp.* **None** rely on the browser **Web Speech API**, which is exactly what Shannon currently stubs. Web Speech is the wrong primitive: unsupported on WebKitGTK (Linux) and WKWebView (macOS), and on Chromium it ships audio to Google. That's why our stub fires on the platforms that matter most.

---

## 2. The three real options for Shannon

### Option A — Cloud STT API (recommended primary)
Capture mic **in Rust**, POST audio to a Whisper-compatible endpoint, insert returned text.

- **Providers** (all OpenAI-compatible `/audio/transcriptions`):
  - **Groq `whisper-large-v3`** — ~**$0.03/hr** ($0.0005/min), sub-second for short prompts. **Best price/latency.**
  - **OpenAI `whisper-1`** — $0.006/min ($0.36/hr). Reliable, ubiquitous.
  - **Deepgram Nova-3** — ~$0.0043/min, **streaming/websocket**, <300ms. Best for *live partial* transcripts.
- **Reuses Shannon's existing provider infra** — if the user already has an OpenAI/Groq key configured, use it; add a small `stt` config block.
- **Works on every platform** (Rust captures audio uniformly; no webview dependency → fixes Linux/macOS immediately).
- **Multilingual** — Whisper covers 99 languages incl. zh-CN (a core Shannon locale). System dictation does not.
- *Cost:* a heavy voice user doing 30 min/day of dictation ≈ **$0.15–0.30/month** on Groq. Negligible.
- *Cons:* needs internet + an API key; audio leaves the device (privacy).

### Option B — Local STT via whisper.cpp sidecar (later, opt-in — matches user steer)
Bundle **whisper.cpp** as a Tauri [sidecar](https://v2.tauri.app/develop/sidecar/) binary; record mic → feed 16 kHz mono PCM → transcript.

- Models: `ggml-base.en` (~39 MB) up to `medium` (~1.5 GB); Metal/CoreML accel on Mac, CUDA on Win/Linux, plain CPU elsewhere.
- **100% offline, private, free.** Existing community plugin: `tauri-plugin-stt` (whisper.cpp-backed).
- *Cons:* bigger install/download, model management, hardware-dependent latency, more packaging complexity. → **Rightfully "later phase."**

### Option C — Web Speech API (current) — **reject**
Platform-inconsistent, unsupported on the dominant desktop webviews, privacy leak on Chromium, and the source of the current stub. Remove it.

---

## 3. Recommendation

> **Phase 1 (ship): Option A — cloud Whisper, Groq-default / OpenAI-fallback, audio captured in Rust, provider key reused from existing config.**
> **Phase 2 (opt-in, later): Option B — whisper.cpp sidecar for offline/privacy, gated behind a setting with model download.**
> **Remove Option C (the Web Speech stub) as part of Phase 1.**

**Why this fits Shannon** (general-user, cross-platform, multi-provider "AI workspace"):
- Leverages infra already present (provider keys, config, permission flows).
- Fixes Linux/macOS **today** (Rust audio capture has no webview dependency).
- Multilingual excellence = zh-CN + en (Whisper's strength; OS dictation's weakness).
- Privacy-conscious users get the local path later, on their terms.

---

## 4. Concrete implementation plan (for when approved)

### Architecture
```
MicButton (press) ──▶ Rust: cpal/tauri-plugin-audio-recorder record 16kHz mono
MicButton (release) ─▶ invoke('transcribe_audio', { path, provider, lang })
                         ├─ cloud: HTTP POST → Groq/OpenAI /audio/transcriptions (or Deepgram ws for partials)
                         └─ local (P2): spawn whisper.cpp sidecar
                      ◀─ returns { text }
ChatInput: insert text into textarea (stream partials if Deepgram/Groq-stream)
```

### Phase 1 — Cloud STT (the actual fix)
1. **Rust audio capture**: add `tauri-plugin-audio-recorder` (or `cpal`) → record to a temp WAV/opus file under `~/.shannon/voice/`.
2. **New command** `transcribe_audio(audio_path, provider)` in a `commands_voice.rs`:
   - Build the multipart request from the configured STT provider; call the OpenAI-compatible endpoint; return text. Reuse existing HTTP + key-loading patterns.
3. **Config** (`~/.shannon/desktop/config.json`): add `stt: { provider: 'groq'|'openai'|'deepgram', model?, apiKeyRef? }`. Default: reuse the OpenAI key if set, else prompt for Groq.
4. **Rewrite `useVoice`**: replace Web Speech path with Tauri `invoke('transcribe_audio')`. Keep the `idle/recording/transcribing` state machine + `MicButton`/`VoiceOrb` (they're already good). `supported` → true whenever a provider is configured.
5. **i18n**: rename the `voice.mic.start.aria` "(stub)" suffix → plain "Start voice recording"; add provider/error/permission strings (en + zh-CN, parity).
6. **Permissions**: mic permission via the existing Tauri permission flow; surface denial with a toast (consistent with the codebase's no-silent-failure convention).

### Phase 2 — Local STT (opt-in)
7. Add `stt.provider = 'local'`; bundle whisper.cpp sidecar per-platform; add a model-download manager (base/small default). Reuse the same `transcribe_audio` command with a local branch.

### Phase 3 — TTS (minor)
8. Keep `speechSynthesis`-based `createTtsSpeaker` for output (widely supported, low priority); optional cloud TTS later.

### Verification gates
- tsc clean; vitest (rewrite the 2 `ChatInput` stub tests to mock `transcribe_audio` instead of the Web Speech stub); Rust `cargo test`/`clippy`.
- Manual smoke on Linux (the dev platform) — the whole point is this now works.
- Privacy: document data flow (cloud = audio sent to provider; local = on-device).

---

## 5. Effort & sequencing

| Phase | Scope | Effort | Depends on |
|---|---|---|---|
| **1 — Cloud STT** | Rust capture + `transcribe_audio` + config + `useVoice` rewrite + i18n + tests | M–L | provider key config (exists) |
| **2 — Local STT** | whisper.cpp sidecar + model manager + setting | L | Phase 1 merged |
| **3 — TTS polish** | optional cloud TTS | S | — |

**Recommendation:** approve Phase 1 as the **D4 resolution** (replaces "gate the stub" with "build the real thing"). Defer Phase 2/3. This turns voice from a labeled-experimental stub into a working cross-platform feature consistent with how every credible competitor ships it.
