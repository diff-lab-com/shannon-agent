# shannon-e2e Handoff Document

**Status**: WIP archived (commit `2f39f9b` on `wip/shannon-e2e-transport` branch, 2026-06-30)
**Target**: Implementers of future v0.3+ or alternative transport layers
**Audience**: Rust developers familiar with crypto primitives, plus anyone integrating with `shannon-mobile` (Dart)

---

## 1. Purpose

`shannon-e2e` provides **end-to-end encryption** between a Shannon desktop client and a Shannon mobile client, mediated by an untrusted `shannon-relay` broker. The relay sees only opaque ciphertext; only the two endpoints possess the key material needed to decrypt.

**Why E2E matters**: Without it, the relay can read every keystroke and message. The relay is "dumb, zero-knowledge" by design — this crate is what makes that property meaningful.

---

## 2. Threat Model & Design Constraints

| Concern | Treatment |
|---|---|
| **Passive network observer** | Defeated — wire is AES-256-GCM ciphertext |
| **Compromised relay** | Defeated — relay has no key material |
| **Long-term key compromise (one side)** | **v0.2 NOT defeated** (static-static, no PFS) — see §7 |
| **Active MITM (insert/reorder)** | Defeated — AES-GCM auth tag, monotonic counter |
| **Replay** | Defeated — receiver tracks last-seen counter per direction |
| **Compromised long-term key + active MITM** | **v0.2 NOT defeated** — v0.3 ephemeral keys fix this |

---

## 3. Protocol Spec (byte-exact, cross-language)

### 3.1 Wire Frame (C6)

```
┌─────────┬──────────────┬──────────────────────────────────────┐
│ ver (1) │ counter (8)   │ AES-256-GCM ciphertext + 16B tag    │
└─────────┴──────────────┴──────────────────────────────────────┘
  0x01       BE u64        variable, ≥16 bytes
```

- `ver` = `0x01` (reserved for future AEAD upgrades)
- `counter` = 8 bytes big-endian, **monotonic per sender direction**, starts at 0 = "no messages yet"; first `seal()` uses counter=1
- `ciphertext + tag` = AES-256-GCM output, tag is the last 16 bytes

### 3.2 AES-GCM Nonce

```
┌────────────┬──────────────────────┐
│ 4 zero B   │ counter (8 BE)       │
└────────────┴──────────────────────┘
       12-byte nonce
```

NIST SP 800-38D 96-bit nonce construction. Uniqueness = counter uniqueness. **Re-key before 2^32 messages per key** (per RFC 5288).

### 3.3 Session Key Derivation

```
shared_secret = X25519(my_secret, peer_public)       # 32 bytes
session_key  = HKDF-SHA256(
                   salt = b"shannon-relay",
                   info = b"shannon-e2e-v1",
                   ikm  = shared_secret,
                   L    = 32,
               )                                        # 32 bytes
```

Both peers derive **the same** session key (X25519 is symmetric). Directionality is provided by independent sender-side counters.

---

## 4. Algorithm Choices

### 4.1 Why these primitives

| Choice | Why this, not alternatives |
|---|---|
| **X25519** (ECDH) | Standard, fast, small (32B keys), well-vetted. Curve25519 family. |
| **HKDF-SHA256** | RFC 5869, standard KDF. SHA-256 over SHA-512 (smaller, sufficient entropy for 32B OKM). |
| **AES-256-GCM** (not XChaCha20-Poly1305) | **Dart `cryptography` and `pointycastle` packages don't have XChaCha20-Poly1305.** AES-GCM is available in both Rust (`aes-gcm`) and Dart. Avoids a new mobile dependency. |
| **Static-static ECDH** (v0.2) | Simpler, no per-session handshake. **Trade-off**: no forward secrecy. |
| **Per-direction 64-bit counter** | 2^64 messages per direction, **unique per key**. Combined with AEAD tag, gives replay protection. |

### 4.2 v0.2 Trade-off (Accepted in D-1)

**No forward secrecy**. If either long-term private key is compromised, **all historical sessions can be decrypted**. This is a deliberate trade-off:

- Pro: No per-session handshake, simpler pairing flow, smaller code
- Pro: Mobile app does not need to do key exchange at every reconnect
- Con: Single key compromise → all past traffic decryptable
- Mitigation: re-pair frequently (rotates keypair), or wait for v0.3

---

## 5. File Layout (current WIP, 13 files, 684 lines)

```
crates/shannon-e2e/
├── Cargo.toml                          # 19 lines, workspace deps
├── src/
│   ├── lib.rs                          # 42 lines, public API surface
│   ├── frame.rs                        # 45 lines, wire format
│   ├── keypair.rs                      # 54 lines, X25519 keypair
│   ├── session.rs                      # 182 lines, AEAD + HKDF + counter
│   └── golden.rs                       # 42 lines, golden vector infrastructure
├── golden/                              # cross-language interop contract
│   ├── empty.hex
│   ├── hello.hex
│   └── binary.hex
└── tests/
    ├── golden_vectors.rs               # 61 lines, byte-exact vector tests
    └── keypair_handshake.rs            # 35 lines, ECDH + derive smoke test
```

---

## 6. Public API Surface

```rust
// keypair.rs
pub type PublicKeyBytes = [u8; 32];
pub struct Keypair {
    pub fn generate() -> Self;
    pub fn from_secret_bytes(secret: PublicKeyBytes) -> Self;
    pub fn public(&self) -> PublicKeyBytes;
    pub fn secret_bytes(&self) -> PublicKeyBytes;
    pub fn session_key_with(&self, peer_public: PublicKeyBytes) -> SessionKey;
}

// session.rs
pub struct SessionKey([u8; 32]);
impl SessionKey {
    pub fn derive(shared_secret: [u8; 32]) -> Self;
    pub fn from_raw(key: [u8; 32]) -> Self;  // tests / golden only
    pub fn channel(&self) -> Channel;
}
impl Drop for SessionKey  // zeroize on drop

pub struct Channel { cipher, counter }
impl Channel {
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, SealError>;
    pub fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>, OpenError>;
}

pub enum SealError { CounterOverflow, ... }
pub enum OpenError { TooShort, Version(u8), Replay{seen, got}, AuthFailed, ... }

// frame.rs
pub const FRAME_VERSION: u8 = 0x01;
pub const HEADER_LEN: usize = 9;
pub const TAG_LEN: usize = 16;
pub const MIN_FRAME_LEN: usize = 25;
pub fn build_frame(counter: u64, ciphertext_and_tag: &[u8]) -> Vec<u8>;
pub fn parse_frame(frame: &[u8]) -> Option<(u8, u64, &[u8])>;
pub fn nonce_for(counter: u64) -> [u8; 12];

// golden.rs (test-only)
pub fn golden(name: &str) -> &'static [u8];
```

---

## 7. Re-implementation Guide

**Estimated cost**: 5-10 days, ~700 lines.

### 7.1 Phase 1: Crate skeleton (1 day)

```bash
cargo new --lib crates/shannon-e2e
```

Add to workspace `Cargo.toml`:
```toml
[workspace.dependencies]
x25519-dalek = { version = "2", features = ["static_secrets"] }
hkdf = "0.12"
sha2 = "0.10"
aes-gcm = "0.10"
rand_core = { version = "0.6", features = ["getrandom"] }
zeroize = { version = "1", features = ["zeroize_derive"] }
thiserror = "1"

[workspace]
members = [..., "crates/shannon-e2e", ...]
```

### 7.2 Phase 2: Public API (1 day)

Copy these from WIP `wip/shannon-e2e-transport` branch:
- `src/lib.rs` (re-exports)
- `src/frame.rs` (verbatim)
- `src/keypair.rs` (verbatim, except `Keypair::generate` may need RNG config update)

### 7.3 Phase 3: Session (2 days)

Copy and adapt `src/session.rs`:
- `SessionKey::derive` — HKDF-SHA256 over the shared secret
- `Channel::seal` — AES-256-GCM encrypt + nonce from counter
- `Channel::open` — AES-256-GCM decrypt + counter check
- `Drop for SessionKey` — zeroize memory on drop

### 7.4 Phase 4: Golden vectors (2 days)

The **most important part** for cross-language interop:

1. Port the 3 `.hex` files from `golden/` to the new crate
2. Implement `golden.rs` test infrastructure
3. Write `tests/golden_vectors.rs` — for each vector, seal then open, assert byte-equal

These vectors are the **authoritative interop contract** with the Dart twin. Any divergence breaks mobile.

### 7.5 Phase 5: Cross-language interop test (1-2 days)

Coordinate with the `shannon-mobile` (Dart) team:
1. Pick one vector (e.g. `hello.hex`)
2. Run seal in Rust, send ciphertext to Dart
3. Dart opens, verifies plaintext match
4. Reverse: Dart seals, Rust opens

If both directions match byte-for-byte → interop verified.

### 7.6 Phase 6: Integration (1-2 days)

- Decide which crate consumes `shannon-e2e`:
  - Likely `shannon-desktop` (host) for outbound
  - Likely `shannon-mobile` (Dart) for inbound
  - `shannon-relay` (zero-knowledge broker) is the **only** integration that needs to **forward** opaque bytes
- Add `shannon-e2e = { path = "../shannon-e2e" }` to consumer `Cargo.toml`
- Wire up: Keypair persistence (host disk / phone secure storage) + Channel lifecycle

---

## 8. Test Strategy

### 8.1 Unit tests
- Frame: `build_frame` ↔ `parse_frame` round-trip
- Nonce: `nonce_for(counter)` produces expected 12-byte output
- HKDF: `SessionKey::derive` is deterministic for same input
- AES-GCM: `seal` ↔ `open` round-trip on known plaintext
- Counter: overflow returns `CounterOverflow`; replay returns `Replay{...}`

### 8.2 Golden vector tests (CRITICAL for interop)

For each vector in `golden/`:

```rust
let key = SessionKey::from_raw(parse_hex("..."));
let plaintext = parse_hex("...");
let expected_frame = parse_hex("...");

let mut channel = key.channel();
let frame = channel.seal(&plaintext).unwrap();
assert_eq!(frame, expected_frame, "vector {name} must match exactly");

let mut channel = key.channel();
let opened = channel.open(&expected_frame).unwrap();
assert_eq!(opened, plaintext);
```

The 3 existing vectors cover: empty message, ASCII "hello", and binary.

### 8.3 Handshake test

```rust
let alice = Keypair::generate();
let bob = Keypair::generate();
let alice_key = alice.session_key_with(bob.public());
let bob_key = bob.session_key_with(alice.public());
assert_eq!(alice_key.as_bytes(), bob_key.as_bytes());
```

### 8.4 Cross-language interop (deferred)

Document the protocol spec in `shannon-mobile`'s `lib/src/crypto/e2e_*.dart` with byte-exact parity. Run end-to-end test on every release.

---

## 9. Known Limitations (must address in v0.3+)

### 9.1 No forward secrecy (D-1)
- **Issue**: Long-term key compromise decrypts all history
- **Fix**: Per-session ephemeral X25519 keys (e.g., `x25519_dalek::EphemeralSecret`)
- **Cost**: 1-2 days, adds handshake overhead

### 9.2 No key rotation
- **Issue**: Single long-term key per (host, phone) pair
- **Fix**: Re-pairing rotates the static keypair; consider time-based rotation

### 9.3 No handshake authentication
- **Issue**: v0.2 trusts the QR-payload to deliver the right public key
- **Fix**: Display short verification code (e.g., first 4 bytes of HKDF output, base32-encoded, both sides must confirm)

### 9.4 No replay window on receiver
- **Issue**: Receiver stores last-seen counter, but doesn't bound the window
- **Fix**: Counter rotation policy, e.g., re-key every 2^32 messages

### 9.5 No MSG-level authentication
- **Issue**: Frame is authenticated, but who sent it is not bound to a public key
- **Fix**: Sign frames with the long-term key (Ed25519 alongside X25519)

### 9.6 Single-key-per-pair
- **Issue**: One compromise = both directions compromised
- **Fix**: Direction-specific subkeys (HKDF info includes "client-to-server" / "server-to-client")

---

## 10. Dependencies (workspace additions needed)

```toml
[workspace.dependencies]
# Existing in workspace
x25519-dalek = { version = "2", features = ["static_secrets"] }  # verify version
hkdf = "0.12"  # may already exist
sha2 = "0.10"  # already a transitive dep
rand_core = { version = "0.6", features = ["getrandom"] }

# New additions needed
aes-gcm = "0.10"
zeroize = { version = "1", features = ["zeroize_derive"] }

# Already in workspace
thiserror = "1"  # likely already present
```

**Verify these versions** against current workspace `Cargo.toml` before adding.

---

## 11. Migration Path (if re-implementing)

```bash
# 1. Re-create the branch from latest main
git checkout main
git pull origin main
git checkout -b feat/shannon-e2e

# 2. Create crate
cargo new --lib crates/shannon-e2e

# 3. Copy the WIP source files (verbatim)
git checkout wip/shannon-e2e-transport -- \
    crates/shannon-e2e/src/ \
    crates/shannon-e2e/tests/ \
    crates/shannon-e2e/golden/ \
    crates/shannon-e2e/Cargo.toml

# 4. Wire up workspace
# Edit root Cargo.toml: add "crates/shannon-e2e" to [workspace] members
# Edit root Cargo.toml: add shannon-e2e deps to [workspace.dependencies]

# 5. Build + test
cargo build -p shannon-e2e
cargo test -p shannon-e2e
cargo clippy -p shannon-e2e -- -D warnings

# 6. Coordinate with shannon-mobile team for interop test
# (See §8.4)

# 7. v0.3 follow-up: implement ephemeral keys
# (See §9.1)
```

---

## 12. References

- **WIP branch**: `wip/shannon-e2e-transport` (commit `2f39f9b`)
- **Workspace**: `shannon-agent/shannon-code` on GitHub
- **Mobile twin**: `shannon-agent/shannon-mobile` (separate repo, Dart)
- **Standards cited**:
  - NIST SP 800-38D (GCM nonce construction)
  - RFC 5869 (HKDF)
  - RFC 7748 (X25519)
  - RFC 5288 (AES-GCM usage in TLS)

---

## 13. Decision Log

| Date | Decision | Reason |
|---|---|---|
| 2026-06-30 | WIP branch created | Isolate e2e work from `api_server approval roundtrip` |
| 2026-06-30 | Chose AES-GCM over XChaCha20-Poly1305 | Dart crypto package compat |
| 2026-06-30 | v0.2: static-static, no PFS | Simpler, defer PFS to v0.3 |
| 2026-06-30 | WIP archived as doc (this file) | User decision: not merging now, document for future |
| TBD | v0.3: ephemeral keys | After mobile integration exists |

---

**Author**: Generated from WIP `2f39f9b` (preserved in branch `wip/shannon-e2e-transport`)
**Last updated**: 2026-07-13
