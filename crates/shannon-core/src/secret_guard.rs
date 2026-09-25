//! Host-side seam for the secret-guard plugin (blueprint artifact **(c)**
//! wiring: `docs/research/llm-secret-redaction-research-2026-09.md` §9).
//!
//! Phase 0 scope: a read-only audit over the serialized outbound request,
//! invoked from the existing request-capture point in the query engine.
//! With no plugin installed this is a no-op — zero behavior change, zero
//! cache impact (the audit never mutates the wire body, so byte-identity
//! with the request on the wire is preserved and prompt caching is safe).
//!
//! Later phases (ingest-time substitution, execution-face restore) use the
//! same seam: install an implementation of
//! [`shannon_plugin_api::ContextTransform`] via [`set_context_transform`].
//! The engine loop, not this module, will drive wiring points 1–3; Phase 0
//! only exercises `audit_wire`.
//!
//! Policy: a plugin is supplied by the host composition (today: tests and
//! embedders; later: the `secret-guard-plugin` crate behind a feature
//! flag/config). The engine itself never constructs a concrete plugin.

use std::sync::{Arc, OnceLock, RwLock};

use shannon_plugin_api::ContextTransform;

static GLOBAL_TRANSFORM: OnceLock<RwLock<Option<Arc<dyn ContextTransform>>>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<Arc<dyn ContextTransform>>> {
    GLOBAL_TRANSFORM.get_or_init(|| RwLock::new(None))
}

/// Install (or remove, with `None`) the process-wide content transform.
pub fn set_context_transform(t: Option<Arc<dyn ContextTransform>>) {
    if let Ok(mut guard) = slot().write() {
        *guard = t;
    }
}

/// The currently installed transform, if any.
pub fn context_transform() -> Option<Arc<dyn ContextTransform>> {
    slot().read().ok().and_then(|g| g.clone())
}

/// Read-only audit of the serialized request body through the installed
/// transform. Returns the number of findings; each finding is logged with
/// its rule id only — never the secret value (the contract's
/// `AuditFinding` carries no secret material by construction).
pub fn audit_wire_and_log(wire: &serde_json::Value) -> usize {
    let Some(transform) = context_transform() else {
        return 0;
    };
    let findings = transform.audit_wire(wire);
    let count = findings.len();
    for f in findings {
        // Deliberately narrow log: rule id only. The wire body itself is
        // already teed into the L0 session log under the redaction policy.
        tracing::warn!(
            target: "shannon::secret_guard",
            rule = %f.rule_id,
            "potential secret detected in outbound LLM request (value redacted)"
        );
    }
    count
}

/// ---- Phase 2 wiring points (blueprint §9.6) -------------------------------
///
/// The engine drives these at the outgoing-send boundary and the tool
/// execution boundary. Both are no-ops until a plugin is installed via
/// [`set_context_transform`].
use shannon_engine::api::types::{ContentBlock, Message, MessageContent, ToolResultContent};
use shannon_plugin_api::{IngestBlock, IngestSource, RestoreAction};

fn ingest_source_for_role(role: &str) -> IngestSource {
    match role {
        "user" => IngestSource::UserMessage,
        _ => IngestSource::Other,
    }
}

fn transform_text(
    t: &dyn shannon_plugin_api::ContextTransform,
    source: IngestSource,
    text: &mut String,
) {
    let mut block = IngestBlock {
        source,
        text: std::mem::take(text),
    };
    let _ = t.transform_ingest(&mut block);
    *text = block.text;
}

fn transform_content_blocks(
    t: &dyn shannon_plugin_api::ContextTransform,
    text_source: IngestSource,
    blocks: &mut [ContentBlock],
) {
    for b in blocks.iter_mut() {
        match b {
            ContentBlock::Text { text } => {
                transform_text(t, text_source.clone(), text);
            }
            ContentBlock::ToolResult { content, .. } => match content {
                Some(ToolResultContent::Single(s)) => {
                    transform_text(t, IngestSource::Other, s);
                }
                Some(ToolResultContent::Multiple(inner)) => {
                    transform_content_blocks(t, IngestSource::Other, inner);
                }
                None => {}
            },
            // `ToolUse` echoes the model's own output (already surrogate-form
            // where applicable); deterministic idempotence (I3) makes touching
            // it unnecessary.
            _ => {}
        }
    }
}

/// Wiring point 1 (send boundary): run every outgoing message through the
/// installed transform. Deterministic (I1) + idempotent (I3) transforms keep
/// the request prefix byte-stable across turns, so provider prompt caching
/// is unaffected. Returns the input vector unchanged when no transform is
/// installed.
pub fn transform_outgoing_messages(messages: Vec<Message>) -> Vec<Message> {
    let Some(t) = context_transform() else {
        return messages;
    };
    messages
        .into_iter()
        .map(|mut m| {
            match &mut m.content {
                MessageContent::Text(text) => {
                    transform_text(t.as_ref(), ingest_source_for_role(&m.role), text);
                }
                MessageContent::Blocks(blocks) => {
                    transform_content_blocks(t.as_ref(), ingest_source_for_role(&m.role), blocks);
                }
            }
            m
        })
        .collect()
}

/// Wiring point 2 (tool execution boundary): the model echoes surrogates
/// into tool arguments; restore real values here, on the execution face
/// only. Never persist the restored input back into conversation history.
/// `Err(reason)` means the plugin failed under `FailMode::Closed` — the
/// caller must refuse execution with a tool-level error.
pub fn restore_tool_args_for_execution(
    tool: &str,
    input: &mut serde_json::Value,
) -> Result<(), String> {
    let Some(t) = context_transform() else {
        return Ok(());
    };
    match t.restore_tool_args(tool, input) {
        RestoreAction::Unchanged => Ok(()),
        RestoreAction::Restored(stats) => {
            for token in &stats.unresolved {
                tracing::warn!(
                    target: "shannon::secret_guard",
                    tool,
                    token = %token,
                    "unresolved placeholder in tool arguments (possible F3/F4); passing through to execution"
                );
            }
            Ok(())
        }
        RestoreAction::Failed { reason } => Err(reason),
    }
}

/// Wiring point 3 (display boundary, blueprint §5.5): restore real values in
/// text about to be shown to the user. No-op when no transform is installed.
/// Callers must apply this to the emitted copy only — the conversation
/// history keeps surrogates (I2).
pub fn restore_display_for_output(text: &mut String) {
    if let Some(t) = context_transform() {
        let _ = t.restore_display(text);
    }
}

/// Streaming display-face restore (wiring point 3 for streamed output).
///
/// Restoring per-delta misses surrogate tokens that arrive split across
/// deltas — a 20-char token spans several model tokens, so streamed output
/// showed raw `SG1:…` placeholders to the user. The restorer feeds deltas
/// through a carry buffer: complete tokens are restored immediately, and
/// only a tail that is (or could grow into) a token is held back. [`Self::finish`]
/// flushes the remainder at end of stream.
///
/// Built around the built-in guard's `SG1:` token shape; other shapes are
/// simply passed through the same hold-back window (complete `SG1:`-formed
/// tokens always restore, partial ones wait for `finish`).
#[derive(Default)]
pub struct DisplayRestorer {
    buf: String,
}

/// `"SG1:"` lead of the built-in surrogate token.
const SG1_LEAD: &str = "SG1:";
/// Full built-in token length: lead + 16 base32 chars (10 HMAC bytes).
const SG1_TOKEN_LEN: usize = 20;

impl DisplayRestorer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one streamed delta; returns the prefix safe to emit now
    /// (already restored). Empty output means everything is still held.
    pub fn feed(&mut self, delta: &str) -> String {
        self.buf.push_str(delta);
        let cut = self.safe_cut();
        let mut out = std::mem::take(&mut self.buf);
        self.buf = out.split_off(cut);
        if !out.is_empty() {
            restore_display_for_output(&mut out);
        }
        out
    }

    /// Flush at end of stream: restore and return everything still held.
    pub fn finish(&mut self) -> String {
        let mut out = std::mem::take(&mut self.buf);
        if !out.is_empty() {
            restore_display_for_output(&mut out);
        }
        out
    }

    /// How much of the buffer is safe to emit: everything up to an
    /// incomplete token, minus any trailing interrupted `SG1:` lead.
    fn safe_cut(&self) -> usize {
        let cut = match self.buf.rfind(SG1_LEAD) {
            Some(i) if self.buf.len() - i < SG1_TOKEN_LEN => i,
            _ => self.buf.len(),
        };
        cut.min(self.len_minus_partial_lead())
    }

    /// Buffer length after dropping a trailing proper prefix of `"SG1:"` —
    /// an interrupted lead must wait for the next delta (or `finish`).
    fn len_minus_partial_lead(&self) -> usize {
        let max = SG1_LEAD.len().min(self.buf.len());
        for keep in (1..=max).rev() {
            if self.buf.ends_with(&SG1_LEAD[..keep]) {
                return self.buf.len() - keep;
            }
        }
        self.buf.len()
    }
}

// ---- Phase 2 productization: built-in host transform (T6) -----------------
///
/// `SHANNON_SECRET_GUARD=audit|redact shannon …` enables the built-in
/// transform for the process. Detection reuses the session-log
/// [`crate::session_log::redaction::RedactionPolicy`] sources (built-in
/// token shapes + `redaction.toml` + env snapshot); surrogates are SG1
/// (HMAC-SHA256, format-compatible with the `secret-guard` artifact-a
/// crate). The external `secret-guard-plugin` remains the full-fidelity
/// implementation (gitleaks corpus); this built-in keeps the default
/// install dependency-free. Enablement is process-wide and one-shot.
use hmac::Mac as _;
use sha2::Digest as _;
use shannon_plugin_api::{AuditFinding, RestoreStats, TransformAction};

/// Outbound policy for the built-in guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretGuardMode {
    /// Detect and count only (default posture for `audit`).
    Audit,
    /// Replace detected secrets with deterministic surrogates at ingest.
    Redact,
}

impl SecretGuardMode {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "audit" => Some(Self::Audit),
            "redact" => Some(Self::Redact),
            _ => None,
        }
    }
}

/// Format-compatible SG1 derivation (see `secret-guard` artifact a):
/// `SG1:` + base32(HMAC-SHA256(master, "sg1:v1\0" || secret)[..10]).
fn sg1_surrogate(secret: &str, master: &[u8]) -> String {
    type HmacSha256 = hmac::Hmac<sha2::Sha256>;
    let mut data = Vec::with_capacity(secret.len() + 8);
    data.extend_from_slice(b"sg1:v1\0");
    data.extend_from_slice(secret.as_bytes());
    let mac = HmacSha256::new_from_slice(master)
        .map(|mut m| {
            m.update(&data);
            m.finalize().into_bytes()
        })
        .unwrap_or_else(|_| sha2::Sha256::digest(&data));
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::from("SG1:");
    let mut acc: u32 = 0;
    let mut acc_bits: u32 = 0;
    for b in &mac[..10] {
        acc = (acc << 8) | u32::from(*b);
        acc_bits += 8;
        while acc_bits >= 5 {
            acc_bits -= 5;
            out.push(ALPHABET[((acc >> acc_bits) & 0x1f) as usize] as char);
        }
    }
    out
}

/// Built-in host implementation of the content-transform contract.
pub struct HostSecretGuard {
    master_key: Vec<u8>,
    redact: bool,
    exact_values: Vec<String>,
    store: std::sync::Arc<dyn shannon_plugin_api::SurrogateStore>,
}

impl HostSecretGuard {
    /// Default constructor over the in-memory store (pilot posture).
    /// `exact_values` seeds known secrets (blueprint §5.7 L2: env snapshot +
    /// redaction.toml declared values — exact-match, no regex involved).
    pub fn new(master_key: Vec<u8>, exact_values: Vec<String>, redact: bool) -> Self {
        Self::with_store(
            master_key,
            exact_values,
            redact,
            std::sync::Arc::new(shannon_plugin_api::InMemorySurrogateStore::default()),
        )
    }

    /// Host-injected registry (T8 seam): a persistent/rebuildable store
    /// owned by the host; the derivation stays stateless regardless.
    pub fn with_store(
        master_key: Vec<u8>,
        exact_values: Vec<String>,
        redact: bool,
        store: std::sync::Arc<dyn shannon_plugin_api::SurrogateStore>,
    ) -> Self {
        let mut exact_values = exact_values;
        // Longest-first so a value nested inside another is replaced first
        // (F5); sorted once here, not on every redaction call.
        exact_values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        Self {
            master_key,
            redact,
            exact_values,
            store,
        }
    }

    fn surrogate_of(&self, secret: &str) -> String {
        sg1_surrogate(secret, &self.master_key)
    }

    fn redact_text(&self, text: &mut String) -> bool {
        let mut changed = false;
        // Exact known values first, longest first (F5: substring nesting).
        // `exact_values` is pre-sorted longest-first at construction, not
        // per call.
        for value in &self.exact_values {
            if text.contains(value.as_str()) {
                let token = self.surrogate_of(value);
                self.store.register(&token, value);
                *text = text.replace(value.as_str(), &token);
                changed = true;
            }
        }
        // Built-in token shapes (sk- / ghp_ / xox / glpat- …) on the result.
        // Every finding is replaced, unconditionally: the store's only job is
        // the token→secret mapping and registration is idempotent. Gating the
        // replacement on "already registered" leaked the raw secret on every
        // send after the first (history re-transforms the same raw text each
        // turn) and flipped the wire bytes against the prompt cache.
        let findings: Vec<String> = crate::session_log::redaction::BUILTIN_PREFIX_REGEX
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .collect();
        for secret in findings {
            let token = self.surrogate_of(&secret);
            self.store.register(&token, &secret);
            *text = text.replace(&secret, &token);
            changed = true;
        }
        changed
    }

    fn restore_text(&self, text: &mut String) -> RestoreAction {
        let mut pairs = self.store.pairs();
        if pairs.is_empty() {
            return RestoreAction::Unchanged;
        }
        pairs.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
        let mut replaced = 0usize;
        for (token, real) in &pairs {
            let hits = text.matches(token.as_str()).count();
            if hits > 0 {
                *text = text.replace(token.as_str(), real);
                replaced += hits;
            }
        }
        if replaced == 0 {
            RestoreAction::Unchanged
        } else {
            RestoreAction::Restored(RestoreStats {
                replaced,
                fuzzy: 0,
                unresolved: Vec::new(),
            })
        }
    }
}

impl ContextTransform for HostSecretGuard {
    fn transform_ingest(&self, block: &mut shannon_plugin_api::IngestBlock) -> TransformAction {
        if !self.redact {
            return TransformAction::Passthrough; // audit mode never modifies
        }
        if self.redact_text(&mut block.text) {
            TransformAction::Modified
        } else {
            TransformAction::Passthrough
        }
    }

    fn restore_tool_args(&self, _tool: &str, args: &mut serde_json::Value) -> RestoreAction {
        let pairs = self.store.pairs();
        if pairs.is_empty() {
            return RestoreAction::Unchanged;
        }
        let mut stats = RestoreStats {
            replaced: 0,
            fuzzy: 0,
            unresolved: Vec::new(),
        };
        walk_restore(args, &pairs, &mut stats);
        if stats.replaced == 0 {
            RestoreAction::Unchanged
        } else {
            RestoreAction::Restored(stats)
        }
    }

    fn restore_display(&self, text: &mut String) -> RestoreAction {
        self.restore_text(text)
    }

    fn audit_wire(&self, wire: &serde_json::Value) -> Vec<AuditFinding> {
        let body = serde_json::to_string(wire).unwrap_or_default();
        let mut findings = Vec::new();
        if crate::session_log::redaction::BUILTIN_PREFIX_REGEX.is_match(&body) {
            findings.push(AuditFinding {
                rule_id: "builtin-shape".to_string(),
            });
        }
        for v in &self.exact_values {
            if body.contains(v.as_str()) {
                findings.push(AuditFinding {
                    rule_id: "env-value".to_string(),
                });
                break;
            }
        }
        findings
    }
}

fn walk_restore(v: &mut serde_json::Value, pairs: &[(String, String)], stats: &mut RestoreStats) {
    match v {
        serde_json::Value::String(s) => {
            for (token, real) in pairs {
                if s.contains(token.as_str()) {
                    *s = s.replace(token.as_str(), real);
                    stats.replaced += 1;
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| walk_restore(x, pairs, stats)),
        serde_json::Value::Object(m) => m.values_mut().for_each(|x| walk_restore(x, pairs, stats)),
        _ => {}
    }
}

fn shannon_home() -> std::path::PathBuf {
    std::env::var_os("SHANNON_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".shannon")
        })
}

/// Load (or create, `0600`) the per-machine master key at
/// `<shannon-home>/secret_guard.key`. The key is the only durable state;
/// the surrogate registry is rebuildable from local secret sources.
///
/// A missing key is created silently; a corrupt one is regenerated **with a
/// warning** — every previously minted surrogate changes, so persisted
/// history can no longer be restored and provider prompt caches invalidate.
fn load_or_create_key(home: &std::path::Path) -> Option<Vec<u8>> {
    let path = home.join("secret_guard.key");
    if let Ok(hex) = std::fs::read_to_string(&path) {
        let hex = hex.trim();
        if hex.len() == 64 {
            if let Some(key) = decode_hex(hex) {
                return Some(key);
            }
        }
        tracing::warn!(
            target: "shannon::secret_guard",
            path = %path.display(),
            "unreadable secret_guard.key — regenerating; previously minted \
             surrogates become unrestorable and prompt caches will miss"
        );
    }
    // 4 random UUIDs (122 random bits each) → SHA-256 → 32 key bytes.
    let mut raw = String::new();
    for _ in 0..4 {
        raw.push_str(&uuid::Uuid::new_v4().simple().to_string());
    }
    let key: [u8; 32] = sha2::Sha256::digest(raw.as_bytes()).into();
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(home).ok();
    if let Err(e) = std::fs::write(&path, &hex) {
        tracing::warn!(
            target: "shannon::secret_guard",
            path = %path.display(),
            error = %e,
            "cannot persist secret_guard.key — secret guard stays disabled \
             (surrogates would not survive a restart)"
        );
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).ok();
    }
    Some(key.to_vec())
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    s.as_bytes()
        .chunks(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
        .collect()
}

static ENABLED: std::sync::OnceLock<SecretGuardMode> = std::sync::OnceLock::new();

/// Parse `$SHANNON_SECRET_GUARD` (`audit` | `redact`; anything else = off).
fn mode_from_env() -> Option<SecretGuardMode> {
    std::env::var("SHANNON_SECRET_GUARD")
        .ok()
        .and_then(|v| SecretGuardMode::parse(&v))
}

/// Install the built-in guard when `$SHANNON_SECRET_GUARD` requests it.
/// One-shot per process (subsequent calls are cheap no-ops). Returns the
/// active mode when installed (or previously installed).
pub fn init_from_env() -> Option<SecretGuardMode> {
    let mode = mode_from_env()?;
    if let Some(existing) = ENABLED.get() {
        return Some(*existing);
    }
    let key = load_or_create_key(&shannon_home())?;
    let exact = crate::session_log::redaction::global_policy()
        .exact_values()
        .to_vec();
    let guard = HostSecretGuard::new(key, exact, mode == SecretGuardMode::Redact);
    set_context_transform(Some(std::sync::Arc::new(guard)));
    let _ = ENABLED.set(mode);
    tracing::info!(target: "shannon::secret_guard", ?mode, "secret-guard enabled (built-in transform)");
    Some(mode)
}

pub fn init_from_config(
    cfg: Option<&crate::unified_config::SecretGuardSection>,
) -> Option<SecretGuardMode> {
    let mode = cfg
        .and_then(|c| c.mode.as_deref())
        .and_then(SecretGuardMode::parse)?;
    if let Some(existing) = ENABLED.get() {
        return Some(*existing);
    }
    let key = load_or_create_key(&shannon_home())?;
    let exact = crate::session_log::redaction::global_policy()
        .exact_values()
        .to_vec();
    let guard = HostSecretGuard::new(key, exact, mode == SecretGuardMode::Redact);
    set_context_transform(Some(std::sync::Arc::new(guard)));
    let _ = ENABLED.set(mode);
    tracing::info!(target: "shannon::secret_guard", ?mode, "secret-guard enabled (built-in transform)");
    Some(mode)
}

#[cfg(test)]
/// Serializes tests that mutate the process-global transform. Shared with
/// other crates' test modules in this workspace member (e.g. tools.rs
/// execution-boundary tests).
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub fn acquire() -> MutexGuard<'static, ()> {
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::acquire as global_lock;
    use super::*;
    use serde_json::json;
    use shannon_plugin_api::{AuditFinding, IngestBlock, RestoreAction, TransformAction};

    const SECRET: &str = "AKIAIOSFODNN7EXAMPLE";
    const TOKEN: &str = "SG1:FAKEFAKEFAKEFAKE";

    /// Walks JSON strings replacing TOKEN→SECRET on restore.
    struct RoundTrip;

    fn walk(v: &mut serde_json::Value, from: &str, to: &str) -> bool {
        match v {
            serde_json::Value::String(s) => {
                if s.contains(from) {
                    *s = s.replace(from, to);
                    true
                } else {
                    false
                }
            }
            serde_json::Value::Array(a) => a.iter_mut().any(|x| walk(x, from, to)),
            serde_json::Value::Object(m) => m.values_mut().any(|x| walk(x, from, to)),
            _ => false,
        }
    }

    impl shannon_plugin_api::ContextTransform for RoundTrip {
        fn transform_ingest(&self, block: &mut IngestBlock) -> TransformAction {
            if block.text.contains(SECRET) {
                block.text = block.text.replace(SECRET, TOKEN);
                TransformAction::Modified
            } else {
                TransformAction::Passthrough
            }
        }
        fn restore_tool_args(&self, _tool: &str, args: &mut serde_json::Value) -> RestoreAction {
            if walk(args, TOKEN, SECRET) {
                RestoreAction::Restored(shannon_plugin_api::RestoreStats {
                    replaced: 1,
                    fuzzy: 0,
                    unresolved: Vec::new(),
                })
            } else {
                RestoreAction::Unchanged
            }
        }
        fn restore_display(&self, _text: &mut String) -> RestoreAction {
            RestoreAction::Unchanged
        }
        fn audit_wire(&self, _wire: &serde_json::Value) -> Vec<AuditFinding> {
            vec![AuditFinding {
                rule_id: "aws-access-token".to_string(),
            }]
        }
    }

    struct Failing;

    impl shannon_plugin_api::ContextTransform for Failing {
        fn transform_ingest(&self, _block: &mut IngestBlock) -> TransformAction {
            TransformAction::Passthrough
        }
        fn restore_tool_args(&self, _tool: &str, _args: &mut serde_json::Value) -> RestoreAction {
            RestoreAction::Failed {
                reason: "locked".to_string(),
            }
        }
        fn restore_display(&self, _text: &mut String) -> RestoreAction {
            RestoreAction::Unchanged
        }
        fn audit_wire(&self, _wire: &serde_json::Value) -> Vec<AuditFinding> {
            Vec::new()
        }
    }

    #[test]
    fn outgoing_messages_noop_without_plugin() {
        let _g = global_lock();
        set_context_transform(None);
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text(format!("key={SECRET}")),
        }];
        let out = transform_outgoing_messages(messages);
        match &out[0].content {
            MessageContent::Text(t) => assert_eq!(t, &format!("key={SECRET}")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn outgoing_messages_transform_text_and_tool_results() {
        let _g = global_lock();
        set_context_transform(Some(std::sync::Arc::new(RoundTrip)));
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text(format!("key={SECRET}")),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: Some(ToolResultContent::Single(format!("value {SECRET}"))),
                    is_error: None,
                }]),
            },
        ];
        let out = transform_outgoing_messages(messages);
        set_context_transform(None);
        match &out[0].content {
            MessageContent::Text(t) => assert!(!t.contains(SECRET), "raw secret on wire: {t}"),
            other => panic!("unexpected {other:?}"),
        }
        match &out[1].content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlock::ToolResult { content, .. } => match content {
                    Some(ToolResultContent::Single(s)) => {
                        assert!(!s.contains(SECRET), "tool result leaked: {s}");
                    }
                    other => panic!("unexpected {other:?}"),
                },
                other => panic!("unexpected {other:?}"),
            },
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn restore_at_execution_boundary_roundtrips_tool_args() {
        let _g = global_lock();
        set_context_transform(Some(std::sync::Arc::new(RoundTrip)));
        let mut args = json!({ "file_path": "/app/.env", "content": format!("id={TOKEN}") });
        let res = restore_tool_args_for_execution("Write", &mut args);
        set_context_transform(None);
        assert!(res.is_ok());
        assert_eq!(args["content"], format!("id={SECRET}"));
    }

    #[test]
    fn restore_failure_maps_to_err_for_fail_closed() {
        let _g = global_lock();
        set_context_transform(Some(std::sync::Arc::new(Failing)));
        let mut args = json!({ "x": 1 });
        let res = restore_tool_args_for_execution("Bash", &mut args);
        set_context_transform(None);
        assert_eq!(res.expect_err("fail-closed maps to Err"), "locked");
    }

    struct FakeDetector;

    impl ContextTransform for FakeDetector {
        fn transform_ingest(&self, _block: &mut IngestBlock) -> TransformAction {
            TransformAction::Passthrough
        }
        fn restore_tool_args(&self, _tool: &str, _args: &mut serde_json::Value) -> RestoreAction {
            RestoreAction::Unchanged
        }
        fn restore_display(&self, _text: &mut String) -> RestoreAction {
            RestoreAction::Unchanged
        }
        fn audit_wire(&self, _wire: &serde_json::Value) -> Vec<AuditFinding> {
            vec![AuditFinding {
                rule_id: "aws-access-token".to_string(),
            }]
        }
    }

    #[test]
    fn mode_parses_env_values() {
        assert_eq!(
            SecretGuardMode::parse("redact"),
            Some(SecretGuardMode::Redact)
        );
        assert_eq!(
            SecretGuardMode::parse(" Audit "),
            Some(SecretGuardMode::Audit)
        );
        assert_eq!(SecretGuardMode::parse("off"), None);
        assert_eq!(SecretGuardMode::parse(""), None);
    }

    #[test]
    fn host_guard_redacts_builtin_shape_and_restores_roundtrip() {
        let _g = global_lock();
        let guard = HostSecretGuard::new(b"master-key-0123456789abcdef".to_vec(), vec![], true);
        let mut block = shannon_plugin_api::IngestBlock {
            source: IngestSource::ToolResult {
                tool: "Read".to_string(),
            },
            text: "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij".to_string(),
        };
        let action = ContextTransform::transform_ingest(&guard, &mut block);
        assert_eq!(action, TransformAction::Modified);
        assert!(
            !block.text.contains("ghp_ABC"),
            "raw token must be gone: {}",
            block.text
        );
        assert!(block.text.contains("SG1:"));

        let mut display = block.text.clone();
        let act = ContextTransform::restore_display(&guard, &mut display);
        assert!(
            matches!(act, RestoreAction::Restored(_)),
            "expected Restored, got {act:?}"
        );
        assert!(display.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij"));
    }

    #[test]
    fn host_guard_audit_mode_never_modifies_but_wire_reports() {
        let guard = HostSecretGuard::new(b"master-key-0123456789abcdef".to_vec(), vec![], false);
        let mut block = shannon_plugin_api::IngestBlock {
            source: IngestSource::UserMessage,
            text: "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij".to_string(),
        };
        assert_eq!(
            ContextTransform::transform_ingest(&guard, &mut block),
            TransformAction::Passthrough
        );
        assert!(
            block.text.contains("ghp_ABC"),
            "audit mode must not touch content"
        );

        let wire = json!({ "messages": [{ "role": "user", "content": "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij" }] });
        let findings = ContextTransform::audit_wire(&guard, &wire);
        assert!(findings.iter().any(|f| f.rule_id == "builtin-shape"));
    }

    #[test]
    fn host_guard_redacts_declared_exact_values() {
        let guard = HostSecretGuard::new(
            b"master-key-0123456789abcdef".to_vec(),
            vec!["MY-DECLARED-SECRET-VALUE".to_string()],
            true,
        );
        let mut block = shannon_plugin_api::IngestBlock {
            source: IngestSource::UserMessage,
            text: "password = MY-DECLARED-SECRET-VALUE".to_string(),
        };
        assert_eq!(
            ContextTransform::transform_ingest(&guard, &mut block),
            TransformAction::Modified
        );
        assert!(!block.text.contains("MY-DECLARED-SECRET-VALUE"));
        assert!(block.text.contains("SG1:"));
    }

    #[test]
    fn host_guard_injected_store_receives_registrations() {
        // T8 seam: a host-owned store (persistent/rebuildable) must capture
        // everything the guard mints — the guard itself stays stateless.
        #[derive(Default)]
        struct CollectingStore(std::sync::Mutex<Vec<(String, String)>>);
        impl shannon_plugin_api::SurrogateStore for CollectingStore {
            fn register(&self, surrogate: &str, secret: &str) {
                self.0
                    .lock()
                    .unwrap()
                    .push((surrogate.to_string(), secret.to_string()));
            }
            fn pairs(&self) -> Vec<(String, String)> {
                self.0.lock().unwrap().clone()
            }
            fn len(&self) -> usize {
                self.0.lock().unwrap().len()
            }
        }

        use shannon_plugin_api::SurrogateStore as _;
        let store = std::sync::Arc::new(CollectingStore::default());
        let guard = HostSecretGuard::with_store(
            b"master-key-0123456789abcdef".to_vec(),
            vec!["MY-DECLARED-SECRET-VALUE".to_string()],
            true,
            store.clone(),
        );
        let mut block = shannon_plugin_api::IngestBlock {
            source: IngestSource::UserMessage,
            text: "a = MY-DECLARED-SECRET-VALUE; b = ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij"
                .to_string(),
        };
        assert_eq!(
            ContextTransform::transform_ingest(&guard, &mut block),
            TransformAction::Modified
        );
        let pairs = store.pairs();
        assert_eq!(
            pairs.len(),
            2,
            "both detections must land in the injected store"
        );
        assert!(block.text.contains("SG1:"));
    }

    /// Perf regression (soft budget): redacting a ~500 KB tool result with
    /// exact + shape layers must stay far below a user-noticeable pause.
    /// Measured baseline: scan throughput ~379 MiB/s (see artifact-a
    /// benches); the 2 s ceiling is ~100x headroom.
    #[test]
    fn redact_large_payload_within_budget() {
        let guard = HostSecretGuard::new(b"master-key-0123456789abcdef".to_vec(), vec![], true);
        let line = "const padding_line = compute(padding_arg); // ordinary code line\n";
        let secret_line = "token = ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij\n";
        let mut text = String::with_capacity(512 * 1024);
        while text.len() < 512 * 1024 {
            text.push_str(line);
            text.push_str(secret_line);
        }
        let mut block = shannon_plugin_api::IngestBlock {
            source: IngestSource::ToolResult {
                tool: "Read".to_string(),
            },
            text,
        };
        let start = std::time::Instant::now();
        assert_eq!(
            ContextTransform::transform_ingest(&guard, &mut block),
            TransformAction::Modified
        );
        let elapsed = start.elapsed();
        assert!(elapsed.as_secs() < 2, "500 KB redaction took {elapsed:?}");
        assert!(!block.text.contains("ghp_ABC"));
    }

    #[test]
    fn key_file_created_once_and_reloaded() {
        let home = std::env::temp_dir().join(format!(
            "sg-key-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let k1 = load_or_create_key(&home).expect("key created");
        assert_eq!(k1.len(), 32);
        let k2 = load_or_create_key(&home).expect("key reloaded");
        assert_eq!(k1, k2, "same home must yield the same master key");
        assert!(home.join("secret_guard.key").exists());
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn audit_noops_without_plugin_then_reports_findings() {
        let _g = global_lock();
        // Default state: no plugin installed → zero findings, no logs.
        assert_eq!(audit_wire_and_log(&json!({ "messages": [] })), 0);

        set_context_transform(Some(Arc::new(FakeDetector)));
        let n = audit_wire_and_log(&json!({ "messages": [{ "role": "user" }] }));
        // Restore default state first so other tests observe the no-op seam.
        set_context_transform(None);
        assert_eq!(n, 1);
        assert!(context_transform().is_none());
    }

    // ---- Regression: repeated occurrences must keep redacting -------------
    //
    // History keeps raw text and the transform re-runs over the full clone
    // every turn (agent_loop send boundary). A secret seen in turn 1 is
    // therefore scanned again in turn 2; the shape layer must replace it
    // again, byte-identically (I1: prompt-cache prefix stability) — the
    // store lookup must only ever gate registration, never replacement.

    #[test]
    fn repeat_occurrence_redacts_on_every_send() {
        let guard = HostSecretGuard::new(b"master-key-0123456789abcdef".to_vec(), vec![], true);
        let raw = "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij".to_string();

        let mut first = shannon_plugin_api::IngestBlock {
            source: IngestSource::UserMessage,
            text: raw.clone(),
        };
        let _ = ContextTransform::transform_ingest(&guard, &mut first);
        assert!(
            !first.text.contains("ghp_ABC"),
            "turn 1 must redact: {}",
            first.text
        );

        // Turn 2: the same raw text is still in history and is re-transformed.
        let mut second = shannon_plugin_api::IngestBlock {
            source: IngestSource::UserMessage,
            text: raw,
        };
        let _ = ContextTransform::transform_ingest(&guard, &mut second);
        assert!(
            !second.text.contains("ghp_ABC"),
            "turn 2 must also redact (repeat leak): {}",
            second.text
        );
        assert_eq!(
            first.text, second.text,
            "repeat transforms must be byte-identical (I1 cache stability)"
        );
    }

    #[test]
    fn transform_is_idempotent_on_already_transformed_text() {
        let guard = HostSecretGuard::new(b"master-key-0123456789abcdef".to_vec(), vec![], true);
        let mut block = shannon_plugin_api::IngestBlock {
            source: IngestSource::UserMessage,
            text: "k=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij".to_string(),
        };
        let _ = ContextTransform::transform_ingest(&guard, &mut block);
        let once = block.text.clone();
        let _ = ContextTransform::transform_ingest(&guard, &mut block);
        assert_eq!(once, block.text, "I3: re-transform must be a no-op");
    }

    // ---- Regression: word-boundary on the shape layer ----------------------
    //
    // `sk-` must not match inside ordinary words that merely end in "sk"
    // followed by a dash and 8 characters (branch/issue names like
    // task-12345678). In the LLM path a false positive rewrites the text
    // the model sees; in the log path it corrupts ordinary content.

    #[test]
    fn words_ending_in_sk_are_not_secrets() {
        let guard = HostSecretGuard::new(b"master-key-0123456789abcdef".to_vec(), vec![], true);
        let raw = "branch: task-12345678-fix done".to_string();
        let mut block = shannon_plugin_api::IngestBlock {
            source: IngestSource::ToolResult {
                tool: "Bash".to_string(),
            },
            text: raw.clone(),
        };
        let _ = ContextTransform::transform_ingest(&guard, &mut block);
        assert_eq!(block.text, raw, "ordinary text must not be corrupted");
    }

    #[test]
    fn real_keys_after_word_chars_are_still_matched() {
        // A key glued to a word char is ambiguous; after a separator or at
        // string start it must match. Cover start / space / colon / newline.
        let guard = HostSecretGuard::new(b"master-key-0123456789abcdef".to_vec(), vec![], true);
        for raw in [
            "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij".to_string(),
            "key: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij".to_string(),
            "line1\nsk-proj-abcdefgh123456789 line2".to_string(),
        ] {
            let mut block = shannon_plugin_api::IngestBlock {
                source: IngestSource::UserMessage,
                text: raw,
            };
            let _ = ContextTransform::transform_ingest(&guard, &mut block);
            assert!(
                block.text.contains("SG1:"),
                "expected a surrogate for {:?}",
                block.text
            );
        }
    }

    // ---- Regression: provenance on the send boundary -----------------------
    //
    // The contract lets plugins apply per-source policy, so the host must
    // report honest sources: typed user text is UserMessage whether it
    // arrives as plain text or as a Text block inside a user message.

    #[test]
    fn user_text_in_blocks_reports_user_source() {
        let _g = global_lock();
        let seen: Arc<std::sync::Mutex<Vec<IngestSource>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        struct RecordSources(Arc<std::sync::Mutex<Vec<IngestSource>>>);
        impl ContextTransform for RecordSources {
            fn transform_ingest(&self, block: &mut IngestBlock) -> TransformAction {
                self.0.lock().unwrap().push(block.source.clone());
                TransformAction::Passthrough
            }
            fn restore_tool_args(
                &self,
                _tool: &str,
                _args: &mut serde_json::Value,
            ) -> RestoreAction {
                RestoreAction::Unchanged
            }
            fn restore_display(&self, _text: &mut String) -> RestoreAction {
                RestoreAction::Unchanged
            }
            fn audit_wire(&self, _wire: &serde_json::Value) -> Vec<AuditFinding> {
                Vec::new()
            }
        }
        set_context_transform(Some(Arc::new(RecordSources(seen.clone()))));
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::Text {
                text: "typed text".to_string(),
            }]),
        }];
        let _ = transform_outgoing_messages(messages);
        set_context_transform(None);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![IngestSource::UserMessage],
            "Text blocks of a user message must carry UserMessage provenance"
        );
    }

    // ---- DisplayRestorer: streaming display-face restore -------------------
    //
    // Model output streams in deltas and a 20-char surrogate token usually
    // spans several of them; per-delta restore therefore left raw `SG1:…`
    // placeholders in the user-visible text. The restorer must hold back
    // only a possible partial token and restore complete ones immediately.

    fn split_guard() -> (HostSecretGuard, String, &'static str) {
        let guard = HostSecretGuard::new(b"master-key-0123456789abcdef".to_vec(), vec![], true);
        let secret = "sk-split-test-secret-value";
        let mut block = shannon_plugin_api::IngestBlock {
            source: IngestSource::UserMessage,
            text: format!("k={secret}"),
        };
        let _ = ContextTransform::transform_ingest(&guard, &mut block);
        let token = block.text.trim_start_matches("k=").to_string();
        assert!(token.starts_with("SG1:"), "unexpected token {token}");
        (guard, token, secret)
    }

    #[test]
    fn display_restorer_restores_token_split_across_deltas() {
        let _g = global_lock();
        let (guard, token, secret) = split_guard();
        set_context_transform(Some(Arc::new(guard)));
        let mut r = DisplayRestorer::new();
        let mid = token.len() / 2;
        let part1 = r.feed(&format!("see {}", &token[..mid]));
        let part2 = r.feed(&token[mid..]);
        let part3 = r.finish();
        set_context_transform(None);
        let out = format!("{part1}{part2}{part3}");
        assert!(!out.contains("SG1:"), "surrogate reached display: {out}");
        assert_eq!(out, format!("see {secret}"), "split token must restore");
    }

    #[test]
    fn display_restorer_restores_whole_token_immediately() {
        let _g = global_lock();
        let (guard, token, secret) = split_guard();
        set_context_transform(Some(Arc::new(guard)));
        let mut r = DisplayRestorer::new();
        let out = r.feed(&format!("a {token} b"));
        let tail = r.finish();
        set_context_transform(None);
        assert_eq!(format!("{out}{tail}"), format!("a {secret} b"));
    }

    #[test]
    fn display_restorer_passes_plain_text_through_unheld() {
        let _g = global_lock();
        set_context_transform(None);
        let mut r = DisplayRestorer::new();
        assert_eq!(r.feed("hello world"), "hello world");
        assert_eq!(r.finish(), "");
    }

    #[test]
    fn display_restorer_finish_flushes_interrupted_lead() {
        let _g = global_lock();
        set_context_transform(None);
        let mut r = DisplayRestorer::new();
        let emitted = r.feed("wait SG");
        let flushed = r.finish();
        assert_eq!(emitted, "wait ", "a partial SG1: lead must be held back");
        assert_eq!(flushed, "SG", "finish must flush the held tail");
    }
}
