# Security & Privacy

Shannon is local-first: state lives in `~/.shannon/`, and nothing leaves your machine except the model API calls you configure. This page is the map of the security architecture — and the honest list of what is enforced where.

## Credentials

- Provider API keys stay on your machine and are used to talk **directly** to the provider you choose. There is no Shannon middleman server.
- IM channel and integration credentials are stored in the **OS keyring** — never in chat context, never in session logs.
- Webhook deliveries are **HMAC-SHA256 signed** (`X-Shannon-Signature`); verify them on the receiver side.

## Secret redaction (secret-guard)

The `secret-guard` plugin — built on the `shannon-plugin-api` content-transform contract — redacts secrets from **outbound messages before they reach the model**, and restores them when tools execute locally.

Design invariants of the plugin API:

1. **Byte-stable determinism** — identical input yields identical output, so provider prompt caches keep hitting. Security that doesn't cost you your cache.
2. **One-way flow** — transforms apply on the way out, restore on the execution face.
3. **Idempotence** — transforming twice changes nothing further.
4. **Explicit failure** — a failed redaction fails loudly instead of passing content through.

Session-level redaction policies are configurable in `~/.shannon/redaction.toml`.

## Sandboxing

| Platform | Provider |
|---|---|
| Linux | Landlock (+ bubblewrap) |
| macOS | Seatbelt |
| Any | bubblewrap |

File writes can be constrained by manifest-derived sandbox enforcement. The `/sandbox` flag is currently experimental — treat it as a seatbelt, not a perimeter.

## Permissions

- **Rule-based classifier** — pattern matching for known safe/dangerous operations.
- **LLM-assisted classification** — async fallback for ambiguous cases (confidence < 0.7).
- **Profiles** — `strict`, `balanced`, `permissive`, or custom (`.shannon/profiles/*.toml`); select with `SHANNON_PERMISSION_PROFILE`.
- **Precedence** — hard deny > soft deny > allow > explicit intent.
- **Approval workflows** — interactive confirmation for risky operations; high-risk tools (computer use, AppleScript/Shortcuts) confirm **per action**.

## Supply chain

- **Prompt-injection scanning** for skills and MCP server content.
- **Signature verification** for installed skills.
- CI gates: `cargo-deny` (dependency audit) and `cargo-semver-checks` (API stability).

## Telemetry & data flow

- **No telemetry by default.** Any anonymous usage signal is strictly opt-in.
- **Local voice input** (whisper.rs) runs on-device; audio is never uploaded. Cloud STT is available but opt-in.
- Session logs, memory, and configuration all live under `~/.shannon/` on your disk.

## Honest limits

- The secret-guard transform covers engine-mediated model traffic; it cannot redact secrets a tool deliberately writes to a file or sends to a remote host you configured.
- Sandboxing strength varies by platform; Landlock/Seatbelt cover syscall-level file/network policy, not a full VM boundary.
- IM inbound messages are scanned, but social-engineering the *human* who approves an action is out of any software's threat model — keep approval habits tight.
