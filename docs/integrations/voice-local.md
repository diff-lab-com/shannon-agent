# Local Voice (whisper-rs) — P2-5e design

> **Status:** design spec, approved by team lead (P2-5e assignment)
> **Author:** p2-5e-voice-local teammate
> **Branch:** `feat/p2-5e-voice-local` (off `dev`)
> **Scope:** add a local-only STT provider (whisper-rs) to the existing D4 voice
> pipeline. The cloud provider (`transcribe_audio` → Groq/OpenAI/custom) stays
> the default; the local path is opt-in and lives behind a Cargo feature so the
> C++ toolchain is a build-time decision, not a runtime one.

## 1. Why this exists

`desktop/CHANGELOG.md` (D4) called out "Phase 2 (local `whisper.cpp` sidecar,
offline) remains a later deliverable" — and `desktop/claudedocs/voice-input-research-2026-06-29.md`
plus `docs/product-review/D1-voice-mode-design.md` both flag it as the
privacy-first alternative. The current `transcribe_audio` is cloud-only and
sends every utterance off-device. For users on air-gapped machines, in
regulated environments, or who simply want zero-data-egress dictation, that's
a blocker. The user's assignment is to ship that "later deliverable" now.

### Why whisper-rs (Rust bindings) and not a whisper.cpp sidecar

The pre-existing research and D1 design both reference a `whisper.cpp sidecar`
bundled as a Tauri external binary. We deliberately diverge from that:

| Approach | Pros | Cons |
|---|---|---|
| `whisper.cpp` sidecar (research / D1) | No recompile; model is the only download; matches the *Superwhisper / Willow* user model | Cross-platform binary packaging, code-signing, separate CI matrix per OS/arch, IPC surface (stdin/stdout JSON or named pipe) we have to write & test |
| **`whisper-rs` (this design)** | One Rust crate; same `cargo build` pipeline; no per-arch artifact matrix; C++ toolchain already in the dev env; smaller surface area; cheaper to test | Requires a C++ toolchain (cmake, clang/g++) at build time; slower iteration on whisper.cpp upgrades |

`whisper-rs = "0.13"` ships the prebuilt whisper.cpp source under
`vendor/` and builds it as a `build.rs` step. We get a static-link
`libwhisper.a` linked into the desktop binary; the bundled whisper.cpp is
pinned to the `whisper-rs` version, so there's no "model vs. runtime"
version-skew bug class. The cross-platform matrix becomes a `cargo` matrix
(which CI already runs), not a Tauri sidecar matrix.

The performance cost is "first `cargo build` takes longer once" — not a
runtime regression. Inference is identical because the same whisper.cpp code
runs.

## 2. Architecture

### 2.1 Existing pieces we reuse (do NOT rebuild)

- `commands_voice.rs::transcribe_audio` (cloud, base64 in) — stays untouched.
- `commands_voice.rs::get_stt_config` / `save_stt_config` — stays untouched.
- `useVoice` hook's `idle → recording → transcribing → idle` state machine.
- `MicButton` / `VoiceOrb` components and the existing `voice.mic.*` / `voice.error.*` i18n keys.
- The cloud `remoteProvider` (MediaRecorder + base64) is reused as-is for the
  cloud path; the local path is a separate provider.

### 2.2 New pieces (this spec)

```
desktop/
├── Cargo.toml                         # +1 feature, +2 deps
├── src/
│   ├── commands_voice.rs              # + transcribe_audio_local, model list/download commands (cfg-gated)
│   ├── commands_voice_models.rs       # NEW: list / download / delete models, sha256 verify
│   └── config.rs                      # + VoiceLocalConfig (provider toggle, model, auto_download)
└── ui/src/
    ├── lib/tauri-api.ts               # + transcribeAudioLocal, listWhisperModels, downloadWhisperModel, deleteWhisperModel, getVoiceLocalConfig, saveVoiceLocalConfig
    ├── lib/voice/
    │   ├── types.ts                   # + 'local' VoiceProviderKind
    │   ├── factory.ts                 # + 'local' branch
    │   ├── localProvider.ts           # NEW: writes WAV temp file, invokes transcribe_audio_local
    │   └── index.ts                   # re-export
    ├── hooks/useVoice.ts              # + provider option
    ├── components/settings/
    │   └── VoiceLocalSettings.tsx     # NEW: provider toggle, model picker, download/delete, language
    └── components/settings/AdvancedSettings.tsx
                                        # + <VoiceLocalSettings /> under <VoiceSttSettings />
desktop/ui/src/i18n/locales/{en,zh-CN}.json
                                        # + settings.voiceLocal.*, voice.mic.download.*
```

### 2.3 Data flow (local)

```
[user]                  [webview]                  [Rust desktop]                   [fs]
 press Mic   ──▶   useVoice(provider='local')
                       │
                  localProvider.start()
                       │ MediaRecorder captures webm
                  user stops
                       │ blobToBase64 → write WAV to temp
                       │   (we use a deterministic path
                       │    under temp dir, not base64 over IPC)
                       │
                       ▼
              invoke('transcribe_audio_local',
                     { audioPath, model: 'base', language: 'en' })
                                              ───────────▶   load ~/.shannon/models/whisper/base.bin
                                                              (auto-download if missing + cfg allows)
                                                            hound → 16kHz mono PCM f32
                                                            whisper_rs::WhisperContext::new
                                                            FullParams (lang, translate=false)
                                                            state.full_n_segments_from_state
                                                            concat segments → text
                                              ◀───────────   { text, confidence?: f32 }
                       │
                       ▼
              onResult({ transcript })
                       │
                       ▼
              ChatInput inserts into composer
```

The local command takes a *path*, not base64 — base64 over the IPC boundary
is wasteful for full 16kHz mono recordings. The frontend writes the WAV to
a temp file and passes the path; the Rust side reads, decodes, and infers.
The temp file is removed on a best-effort basis (see §5 error handling).

### 2.4 Data flow (model download)

```
Settings → Voice → "Download 'base'"
  │
  ▼
list_available_models()             ◀── hard-coded list with name, filename, size, sha256, url
  │   tiny.en, base, small          (medium/large-v3 are NOT exposed; too big for on-demand)
  ▼
download_model('base')
  │   reqwest::Client stream() → ~/.shannon/models/whisper/base.bin.partial
  │   while downloading:
  │     app.emit('voice:model-download-progress', { model: 'base', progress: 0.42 })
  │   on done:
  │     sha256(base.bin.partial) == expected → rename to base.bin
  │     app.emit('voice:model-download-progress', { model: 'base', progress: 1.0, done: true })
  ▼
UI: re-query listAvailableModels → status flips to "downloaded"
```

## 3. Cargo feature & deps

```toml
# desktop/Cargo.toml
[features]
default = ["tauri"]
tauri   = [...]                       # existing
voice-local = ["dep:whisper-rs", "dep:hound"]

[dependencies]
# Existing reqwest with stream+multipart is reused for the model download.
# whisper-rs 0.13 ships whisper.cpp 1.5.x and exposes a small safe API.
# hound 3 is a minimal pure-Rust WAV reader — used to convert recordings
# (the webview always writes WAV; MediaRecorder webm is not what whisper-rs
#  wants, and bundling an opus decoder for one feature is overkill).
whisper-rs = { version = "0.13", optional = true }
hound      = { version = "3",    optional = true }
```

**Toolchain requirement** — the `voice-local` feature requires a C++ build
chain on the host:

- `cmake` ≥ 3.18 (we have 3.27.2 in the dev env)
- a C++17 compiler (`clang` or `g++`)
- `make` / `ninja`

Documented in the voice-local doc and in the new
`docs/integrations/voice-local.md#setup`. CI must opt into the feature
explicitly (a dedicated `cargo build --features voice-local` job) so the
default `cargo check` / `cargo clippy` runs in this repo stay fast and don't
require the C++ toolchain.

`whisper-rs` carries its own pinned `whisper.cpp` as a git submodule; the
first build pulls + compiles ~150 C++ files. We accept the one-time cost.

**Why optional and not a workspace dep** — the `voice-local` dep is *only*
used by `commands_voice.rs` (and the new `commands_voice_models.rs`).
`whisper-rs` does not build on the `wasm32-unknown-unknown` target either,
so gating it on a feature keeps the `cargo check --all-targets` matrix
fast for contributors who don't care about local STT. The architecture
invariant 1 (workspace metadata separation) is unaffected because no
workspace member `path =`-depends on the desktop crate.

## 4. Whisper model catalog

Hard-coded list — *not* fetched from HuggingFace at runtime. Sizes are
ballpark; we pin the URL + sha256 for each (the catalog version is the
config key, so updating is a deliberate, reviewable change).

| `WhisperModel` | file                   | size     | URL                                                                          | sha256 (placeholder, see §9) |
|----------------|------------------------|----------|------------------------------------------------------------------------------|------------------------------|
| `tiny.en`      | `ggml-tiny.en.bin`     | ~75 MB   | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin` | TBD                          |
| `base`         | `ggml-base.bin`        | ~140 MB  | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin`    | TBD                          |
| `small`        | `ggml-small.bin`       | ~460 MB  | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin`   | TBD                          |

`medium` and `large-v3` are deliberately not exposed: they exceed 1.5 GB
and 3 GB respectively, and our use case (chat composer dictation) is well
served by `base` (multilingual) and `tiny.en` (English, fastest). Users who
need a bigger model can drop a `ggml-*.bin` into `~/.shannon/models/whisper/`
manually; the path-resolution helper picks it up on next launch.

The `WhisperModel` enum is `#[non_exhaustive]` so adding `medium` later
is a one-line change with no breaking wire-shape impact.

```rust
// desktop/src/commands_voice_models.rs (sketch)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "lowercase")]
pub enum WhisperModel {
    #[serde(rename = "tiny.en")]
    TinyEn,
    Base,
    Small,
}
```

## 5. Errors (typed, prefix-stable)

| Rust error string          | Frontend `code`     | When                                                      |
|----------------------------|---------------------|-----------------------------------------------------------|
| `STT_MODEL_NOT_FOUND`      | `model-not-found`   | Model not on disk AND `auto_download=false`               |
| `STT_MODEL_LOADING`        | `model-loading`     | File at the expected path is corrupted / wrong sha        |
| `STT_AUDIO_INVALID`        | `audio-invalid`     | hound couldn't decode the WAV                              |
| `STT_INFERENCE_FAILED`     | `inference-failed`  | whisper-rs returned an error mid-run                      |
| `STT_LANGUAGE_UNSUPPORTED` | `language-unsupported` | `language` hint isn't a valid whisper code             |
| `STT_DOWNLOAD_FAILED`      | `download-failed`   | reqwest stream error / sha256 mismatch after retry        |

The frontend maps these exactly the way `remoteProvider.ts::mapSttError`
already does (extend the switch, don't fork the function).

## 6. Frontend wiring

### 6.1 VoiceProvider kind

`types.ts` adds `'local'` to `VoiceProviderKind`. `factory.ts` adds a case
that builds `localProvider` when kind is `'local'` *and* the runtime
supports it (Tauri webview + `MediaRecorder` present, same gating as
remote). When MediaRecorder is absent, the factory falls back to the stub
provider — same fallback semantics as today.

### 6.2 `useVoice` change

```ts
export interface UseVoiceOptions {
  // ... existing fields
  /** 'cloud' (default) or 'local'. Reads from settings if unset. */
  provider?: 'cloud' | 'local'
}
```

If `provider` is unset, the hook reads `voiceLocalEnabled` from the desktop
config and switches. The factory caches the provider per `useVoice`
instance (no re-binding mid-recording).

### 6.3 `localProvider`

Same MediaRecorder capture loop as `remoteProvider`, with two differences:
1. On stop, instead of base64-encoding, it writes the recording as WAV to
   `path.join(os.tmpdir(), 'shannon-voice-<ts>.wav')` via Tauri's `path`
   plugin + the `fs` plugin. (WAV because the webview's MediaRecorder
   output is `audio/webm` or `audio/ogg` — the Rust side uses hound to
   *read* WAV. If we wanted to accept webm, we'd need an opus decoder. WAV
   is the path of least resistance: 16-bit PCM mono at 16 kHz; the browser
   supports it directly via `MediaRecorder({ mimeType: 'audio/wav' })`
   only on Chromium ≥ 110; the rest of the time we ship the webm bytes
   and the Rust side decodes with a different library. **Decision: ship
   webm from the browser, decode with `hound` if it's WAV, else fail
   with `STT_AUDIO_INVALID`.** Most desktop webviews will be Chromium
   ≥ 110, so WAV works; for the Safari webview case the error message
   is honest.)
2. Calls `transcribeAudioLocal(audioPath, model, language)` instead of
   `transcribeAudio`.

### 6.4 Settings UI

A new `<VoiceLocalSettings />` card, placed under the existing
`<VoiceSttSettings />` in `AdvancedSettings.tsx`. It is **only** mounted
when the `voice-local` Cargo feature was compiled in — detected at runtime
via a `featureFlags` command (a one-liner; cheap). Layout:

- **Provider toggle**: "Use local voice (offline)" — `Switch` bound to
  `voiceLocalEnabled`.
- **Model picker**: `<Select>` with `tiny.en` / `base` / `small`. Disabled
  until a model is downloaded.
- **Download / Delete buttons**: trigger the model commands; show progress
  as a `Progress` bar bound to the `voice:model-download-progress` event.
- **Language hint**: `<Input>` (BCP-47), optional; passed to the inference
  call.
- **Auto-download switch**: default `true`; when off, missing models
  produce a `STT_MODEL_NOT_FOUND` toast with a "Download now" action.

## 7. Storage

- Models: `~/.shannon/models/whisper/<file>.bin` (sha256-verified).
- Settings: extend `DesktopConfig` with:

  ```rust
  pub struct VoiceLocalConfig {
      pub enabled: bool,            // default false (cloud is still default)
      pub model: Option<String>,    // "tiny.en" | "base" | "small"
      pub language: Option<String>, // BCP-47 hint
      pub auto_download: bool,      // default true
  }
  ```

  Persisted via the existing `configure` command (`commands_config::configure`)
  and surfaced as a new typed `save_voice_local_config` / `get_voice_local_config`
  pair (matching the cloud pattern).

## 8. Tests

### Rust (`commands_voice.rs` `mod tests`)

- `stt_endpoint_uses_canonical_defaults` — *existing*, reused.
- `transcribe_audio_local_returns_error_for_missing_model` — uses
  `tempfile::TempDir` + `XDG_CONFIG_HOME` redirect to a clean config dir
  with no models; asserts the error is `STT_MODEL_NOT_FOUND`.
- `transcribe_audio_local_handles_invalid_wav` — writes 4 random bytes to
  a temp file, calls the helper, expects `STT_AUDIO_INVALID`.
- `model_path_resolution_includes_user_dir_override` — resolves
  `~/.shannon/models/whisper/base.bin` and verifies the env override
  changes the path.
- `whisper_model_filename_round_trips` — every variant serializes to the
  expected filename; sha256 is non-zero length.
- `download_progress_event_payload_is_bounded` — if the download helper
  has a pure-IO shape, factor out the progress calculation; otherwise
  leave as a manual integration assertion.

Tests that touch `whisper-rs::WhisperContext` are gated behind the
`voice-local` feature so the default `cargo test` doesn't need a model
file. The non-feature tests cover the wire shape and the I/O plumbing.

### Frontend (`desktop/ui/src/lib/voice/`)

- `factory.test.ts` (extend) — when `kind: 'local'` is passed and
  MediaRecorder is mocked, `createVoiceProvider` returns a local
  provider; when it's absent, it falls back to the stub.
- `localProvider.test.ts` (new) — `transcribeAudioLocal` is invoked with
  the WAV path, the recorded bytes, the chosen model, and the language
  hint. STT_* error prefixes map to the same frontend codes as cloud.
- `useVoice.test.tsx` (extend) — `provider: 'local'` selects the local
  branch; `provider: 'cloud'` (default) keeps the existing behavior.

No real downloads in tests — we mock `downloadWhisperModel` and
`transcribeAudioLocal` in the same way `voice-providers.test.ts` already
mocks `transcribeAudio`.

## 9. Known unknowns (will resolve during implementation)

- **whisper-rs 0.13 full API surface** — I'll verify the exact
  `WhisperContext::new` / `state.full_n_segments_from_state` /
  `state.get_segment` call shape during implementation; the design's
  data-flow is correct regardless of the precise builder method names.
- **sha256 values for the three pinned models** — these are published by
  `ggerganov/whisper.cpp` but verifying them on first run of the test
  is the only way to get the *correct* values; the implementation
  command will write them down at that point. Until then the catalog
  ships with placeholders and the download flow is *correctness-gated*:
  mismatch ⇒ `STT_DOWNLOAD_FAILED`, no install.
- **Whether whisper-rs 0.13 builds clean on this toolchain** — the dev
  env has cmake 3.27.2 + g++ 11.4 + clang + make, which meets whisper-rs's
  documented requirements. If the first build fails, the design falls
  back to *exactly* the sidecar path from the prior research — same
  user model, more CI work. The fallback is documented but not chosen
  in advance.

## 10. Out of scope (deliberate)

- Bundling models with the installer (the `auto_download` flow is the
  install model; installers stay small).
- GPU acceleration (CUDA / Metal) — whisper-rs 0.13 supports it, but
  enabling per-platform needs a separate CI matrix; not in this scope.
  CPU inference is correct, just slower (~real-time on `base` for M1
  class CPUs, ~3× on a modern x86).
- Streaming / partial transcripts (whisper-rs supports them via
  `full_parallel` / segment callbacks) — added in a follow-up if
  latency matters.
- TTS polish (Phase 3 of the prior research) — unchanged.

## 11. Acceptance

The team's acceptance list, in test form:

```text
cargo build -p shannon-desktop                          # default, no voice-local
cargo build -p shannon-desktop --features voice-local   # local path enabled
cargo clippy -p shannon-desktop -- -D warnings          # clean
cargo nextest run -p shannon-desktop                    # all green
pnpm run check && pnpm run lint && pnpm test            # UI green
```

Plus the implicit constraints:

- No new `#[allow(dead_code)]` without a `// KEEP:` marker on the same
  line (architecture invariant 3).
- The cloud `transcribe_audio` path is unchanged — existing tests still
  pass.
- The `voice-local` feature is opt-in; the *default* desktop build is
  smaller and faster.

## 12. Commit + report

Single commit on `feat/p2-5e-voice-local` (no push, no merge):

```
feat(desktop): P2-5e local voice — whisper-rs + model download + UI + settings + tests + docs
```

Files added: `commands_voice_models.rs`, `localProvider.ts`,
`VoiceLocalSettings.tsx`, `voice-local.md`, the two i18n key blocks,
plus the unit tests.

Files modified: `desktop/Cargo.toml`, `commands_voice.rs`,
`commands_config.rs` (or `config.rs` for the new struct),
`commands.rs` (feature-gated re-export), `main.rs` (command registration),
`tauri-api.ts`, `lib/voice/{types,factory,index}.ts`,
`hooks/useVoice.ts`, `components/settings/AdvancedSettings.tsx`,
`desktop/CHANGELOG.md`.
