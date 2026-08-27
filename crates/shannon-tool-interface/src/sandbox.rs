//! Sandbox seam types (§4.12 W3-3b): pluggable execution-world boundaries.
//!
//! A sandbox is an **execution-world boundary** (OS/kernel level). It stacks
//! with — and never replaces — the permission system, which is a **decision
//! layer** over model behavior (`permission/decision` events are unaffected;
//! see master plan §4.12 constraint 4). When a sandbox denies an operation,
//! the denial surfaces as a normal tool error classified
//! [`SANDBOX_DENIED_CLASSIFICATION`] so the L0 event log can record it in the
//! `tool/result` payload's extensible `meta` field without new event kinds.
//!
//! ## Shape
//!
//! - [`SandboxMode`]: the configured switch (`off|local|landlock`), default
//!   `off` (byte-identical to the §4.11 passthrough).
//! - [`SandboxPolicy`]: `{writable_roots, readable_roots, executable_roots,
//!   network}` — the declarative policy a [`SandboxProvider`] enforces.
//! - [`SandboxedFs`](https://docs.rs/shannon-tools) /
//!   [`SandboxedProcess`](https://docs.rs/shannon-tools) (in `shannan-tools`):
//!   decorators implementing [`FileSystemProvider`] / [`ProcessProvider`].
//!   Tool code never learns which world it runs against.
//! - [`SandboxProvider`]: what a *backend* (e.g. Landlock, in
//!   `shannon-tools`) contributes: probe/degrade reporting plus decoration of
//!   concrete worlds.
//!
//! ## Process decoration contract
//!
//! Filesystem denial is portable user-space policy math, but restricting
//! *child processes* needs enforcement that reaches into every fork. Backends
//! therefore decorate worlds whose providers implement [`ForkInitHost`] —
//! a provider able to run a [`ChildWorldInit`] initializer inside each child,
//! immediately before exec (the standard Landlock hook point). Providers that
//! cannot host initializers fail closed with a clear `Err`; a silent fake
//! sandbox is never produced.

use crate::providers::{FileSystemProvider, ProcessProvider};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Classification vocabulary (flows into L0 `tool/result.meta`)
// ---------------------------------------------------------------------------

/// Canonical classification token recorded when a sandbox boundary rejects an
/// operation. Stored in `ToolOutput.metadata["classification"]` and mirrored
/// verbatim into the L0 `tool/result` payload's `meta`.
pub const SANDBOX_DENIED_CLASSIFICATION: &str = "sandbox_denied";

/// Prefix of every sandbox-denial error message, chosen stable so downstream
/// layers (engine metadata derivation, NDJSON consumers) can classify without
/// string-guessing arbitrary OS errors.
pub const SANDBOX_DENIED_PREFIX: &str = "sandbox denied";

/// Structured description of one sandbox rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxDenialInfo {
    /// Operation refused (e.g. `"write"`, `"exec"`).
    pub op: String,
    /// Absolute target path (empty for non-path operations such as TCP bind).
    pub target: String,
    /// Human-readable reason (remediation hint included where useful).
    pub reason: String,
}

impl SandboxDenialInfo {
    /// Render the canonical denial line used inside tool-visible errors.
    pub fn render(&self) -> String {
        format!(
            "{SANDBOX_DENIED_PREFIX}: op={} target={} ({})",
            self.op, self.target, self.reason
        )
    }

    /// Build an `io::Error` carrying this denial with
    /// [`io::ErrorKind::PermissionDenied`], message starting with
    /// [`SANDBOX_DENIED_PREFIX`].
    pub fn into_io_error(self) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, self.render())
    }

    /// Extract a denial from text previously rendered by [`Self::render`].
    ///
    /// Expects exactly `sandbox denied: op=<..> target=<..> (<reason>)`.
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text
            .trim()
            .strip_prefix(SANDBOX_DENIED_PREFIX)?
            .strip_prefix(": op=")?;

        let op_end = rest.find(" target=")?;
        let op = rest[..op_end].to_string();

        let after_target = &rest[op_end + " target=".len()..];
        let reason_open = after_target.rfind(" (")?;
        let close = after_target.rfind(')')?;
        if close < reason_open {
            return None;
        }

        let target = after_target[..reason_open].to_string();
        let reason = after_target[reason_open + 2..close].to_string();

        if op.is_empty() || reason.is_empty() || target.is_empty() {
            return None;
        }
        Some(Self { op, target, reason })
    }

    /// JSON object stored under `ToolOutput.metadata["sandbox"]`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "denied": true,
            "op": self.op,
            "target": self.target,
            "reason": self.reason,
        })
    }
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// Declarative sandbox policy: what the execution world may reach.
///
/// Paths must be absolute and canonicalized by whoever assembles the policy
/// ([`path_within`] matches lexically on components). Grant semantics mirror
/// Landlock's allow-list model: everything not granted is denied once a world
/// actually enforces the policy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxPolicy {
    /// Directories children/in-process tools may modify (granted read+write).
    pub writable_roots: Vec<PathBuf>,
    /// Extra directories readable but not writable (tools usually get the
    /// writable set implicitly here as well).
    pub readable_roots: Vec<PathBuf>,
    /// Directories from which binaries may be executed (`Execute` access);
    /// redundant with writable roots (which always grant execute).
    pub executable_roots: Vec<PathBuf>,
    /// Whether children may create/use TCP sockets. `false` ⇒ connect/bind
    /// are refused wherever the backend supports it (Landlock net ABI on
    /// sufficiently recent kernels).
    pub network: bool,
}

impl SandboxPolicy {
    /// True when `path` sits beneath any granted write root.
    pub fn allows_write(&self, path: &Path) -> bool {
        self.writable_roots
            .iter()
            .any(|root| path_within(path, root))
    }

    /// True when `path` sits beneath any granted root (write roots imply
    /// read).
    pub fn allows_read(&self, path: &Path) -> bool {
        self.writable_roots
            .iter()
            .any(|root| path_within(path, root))
            || self
                .readable_roots
                .iter()
                .any(|root| path_within(path, root))
            || self
                .executable_roots
                .iter()
                .any(|root| path_within(path, root))
    }

    /// True when `path` sits beneath a root granting execute.
    pub fn allows_execute(&self, path: &Path) -> bool {
        self.writable_roots
            .iter()
            .any(|root| path_within(path, root))
            || self
                .executable_roots
                .iter()
                .any(|root| path_within(path, root))
    }

    /// Build the standardized refusal for `op` against `path`.
    pub fn denial_for(&self, op: &str, path: &Path) -> SandboxDenialInfo {
        SandboxDenialInfo {
            op: op.to_string(),
            target: path.display().to_string(),
            reason: format!(
                "outside sandbox roots (writable={:?})",
                self.writable_roots
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
            ),
        }
    }
}

/// Lexical containment check: is `path` equal to or below `root`?
///
/// Component-wise (`Path::strip_prefix`); no symlink traversal happens here —
/// callers owning enforcement canonicalize first, and in-process checks are
/// advisory beside kernel-enforced child worlds anyway.
pub fn path_within(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok()
}

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

/// Configured sandbox switch. Missing configuration resolves to
/// [`SandboxMode::Off`], which keeps the pre-sandbox behavior byte-for-byte.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SandboxMode {
    /// Current default assembly, untouched (§4.11 local passthrough).
    #[default]
    Off,
    /// Explicitly named current-strategy world (no kernel restriction;
    /// existing argv-level wrappers still apply where the platform provides
    /// them). On platforms lacking wrappers this reports availability instead
    /// of pretending to restrict.
    Local,
    /// Kernel-enforced world (Linux Landlock ≥ ABI v1 / kernel 5.13).
    /// Degrades with a loud warning and never silently fake-restricts.
    Landlock,
}

impl SandboxMode {
    /// Parse the configuration token (`off|local|landlock`, case-insensitive).
    pub fn parse(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "local" => Some(Self::Local),
            "landlock" => Some(Self::Landlock),
            _ => None,
        }
    }

    /// Canonical lowercase token (config/debug output).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Local => "local",
            Self::Landlock => "landlock",
        }
    }
}

impl std::fmt::Display for SandboxMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Errors / degrade notices
// ---------------------------------------------------------------------------

/// Sandbox construction or decoration failure. Every variant is loud: no
/// code path turns an unsupported request into a silently-unrestricted run.
#[derive(Error, Debug)]
pub enum SandboxError {
    /// The host lacks the kernel feature (or a required sub-ABI) for the
    /// requested backend.
    #[error("sandbox backend '{backend}' unavailable on this host: {detail}")]
    Unsupported {
        /// Backend identifier, e.g. `"landlock"`.
        backend: String,
        /// Why the backend rejected this host.
        detail: String,
    },
    /// Configuration could not be honored (bad paths, contradictory knobs).
    #[error("invalid sandbox configuration: {0}")]
    InvalidConfig(String),
    /// The provider being decorated cannot host fork-time initializers.
    #[error("process world cannot host sandbox enforcement: {0}")]
    Undecorable(String),
}

/// One recorded degradation discovered while constructing a backend
/// (probe result, clamped feature, skipped rule). Assemblers forward these to
/// logs/events so the operator sees the downgrade explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DegradeNotice {
    /// Stable short reason tag (e.g. `"net-abi-unavailable"`).
    pub tag: String,
    /// Operator-facing explanation including remediation.
    pub detail: String,
}

impl DegradeNotice {
    /// Convenience constructor.
    pub fn new(tag: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            detail: detail.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Provider traits
// ---------------------------------------------------------------------------

/// A fork-time child initializer: runs inside each freshly forked child
/// before exec installs the execution-world boundary (Landlock rulesets are
/// installed exactly here). Returning `Err` aborts the spawn — the child can
/// never escape by skipping its boundary.
pub trait ChildWorldInit: Send + Sync {
    /// Install the world inside the current (forked) child.
    fn init_child(&self) -> io::Result<()>;
}

/// Capability marker for process providers able to host a
/// [`ChildWorldInit`] between fork and exec.
///
/// Implemented by local-world providers (`LocalProcess`); remote/other-world
/// providers embed their own boundary naturally, so they simply do not
/// implement this — backends then fail closed via
/// [`SandboxError::Undecorable`]-style reporting rather than wrapping nothing.
pub trait ForkInitHost: ProcessProvider {
    /// Produce a provider equivalent to `self` where every future child runs
    /// `init` immediately before exec. Returns `Err` on platforms/engines
    /// unable to honor the hook (fail-closed).
    fn boxed_with_fork_init(
        self: Arc<Self>,
        init: Arc<dyn ChildWorldInit>,
    ) -> Result<Arc<dyn ProcessProvider>, String>;
}

/// A sandbox *backend*: probes the host, exposes its policy, and decorates
/// concrete execution worlds so tools run inside the restricted environment
/// without source changes.
pub trait SandboxProvider: Send + Sync + 'static {
    /// Backend identifier (`"local"`, `"landlock"`, …) for logs/metrics.
    fn kind(&self) -> &'static str;

    /// The enforced policy snapshot.
    fn policy(&self) -> &SandboxPolicy;

    /// Non-fatal downgrades discovered at construction time (already logged;
    /// surfaced again for callers building structured events).
    fn degrade_notices(&self) -> &[DegradeNotice];

    /// Wrap a filesystem world: operations outside the policy are rejected
    /// with [`SandboxDenialInfo`]-classified errors; everything else forwards
    /// byte-transparently to `inner`.
    fn decorate_fs(&self, inner: Arc<dyn FileSystemProvider>) -> Arc<dyn FileSystemProvider>;

    /// Wrap a fork-capable process world. Enforcement lives in the fork hook
    /// installed via [`ForkInitHost`]; the returned wrapper preserves
    /// stream bytes exactly and delegates all spawns to it.
    fn decorate_process(
        &self,
        inner: Arc<dyn ForkInitHost>,
    ) -> Result<Arc<dyn ProcessProvider>, SandboxError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> SandboxPolicy {
        SandboxPolicy {
            writable_roots: vec![PathBuf::from("/ws")],
            readable_roots: vec![PathBuf::from("/docs")],
            executable_roots: vec![PathBuf::from("/usr")],
            network: false,
        }
    }

    #[test]
    fn policy_path_math_grants_and_denies() {
        let p = policy();
        assert!(p.allows_write(Path::new("/ws/a/b.txt")));
        assert!(p.allows_write(Path::new("/ws"))); // root itself writable
        assert!(!p.allows_write(Path::new("/etc/passwd")));
        assert!(!p.allows_write(Path::new("/wsX/escape"))); // component boundary
        assert!(p.allows_read(Path::new("/docs/spec.md")));
        assert!(p.allows_read(Path::new("/usr/bin/bash")));
        assert!(p.allows_execute(Path::new("/usr/bin/bash")));
        assert!(!p.allows_execute(Path::new("/docs/tool.sh")));
    }

    #[test]
    fn mode_parse_roundtrip_case_insensitive() {
        for (token, expected) in [
            ("off", SandboxMode::Off),
            ("LANDLOCK", SandboxMode::Landlock),
            (" local ", SandboxMode::Local),
        ] {
            assert_eq!(SandboxMode::parse(token), Some(expected));
            assert_eq!(expected.as_str(), expected.to_string());
        }
        assert_eq!(SandboxMode::parse("bwrap"), None);
        assert_eq!(SandboxMode::default(), SandboxMode::Off);
    }

    #[test]
    fn denial_render_parse_roundtrip_is_stable() {
        let info = SandboxDenialInfo {
            op: "write".into(),
            target: "/etc/shadow".into(),
            reason: "outside sandbox roots".into(),
        };
        let text = info.render();
        assert!(text.starts_with(SANDBOX_DENIED_PREFIX));
        let parsed = SandboxDenialInfo::parse(&text).expect("roundtrip");
        assert_eq!(parsed.op, "write");
        assert_eq!(parsed.target, "/etc/shadow");
        assert_eq!(parsed.reason, "outside sandbox roots");
        assert_eq!(parsed.to_json()["classification"].as_str(), None);

        let err = info.clone().into_io_error();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(err.to_string().starts_with(SANDBOX_DENIED_PREFIX));
    }

    #[test]
    fn denial_parser_rejects_non_sandbox_text() {
        assert!(SandboxDenialInfo::parse("Permission denied").is_none());
        assert!(SandboxDenialInfo::parse("sandbox denied: garbage").is_none());
        assert!(SandboxDenialInfo::parse("sandbox denied: op=write target=/x (").is_none());
    }

    #[test]
    fn denial_metadata_shape_for_l0_meta() {
        let info = SandboxDenialInfo {
            op: "tcp_connect".into(),
            target: String::new(),
            reason: "network disabled in sandbox".into(),
        };
        let json = info.to_json();
        assert_eq!(json["denied"], serde_json::Value::Bool(true));
        assert_eq!(json["op"], serde_json::Value::String("tcp_connect".into()));
    }

    /// The decorator contract is exercisable against the bare traits: any
    /// [`ForkInitHost`] can be wrapped into a plain [`ProcessProvider`]
    /// through its capability method. This pins the seam shape that
    /// shannon-tools' decorators rely on.
    #[test]
    fn fork_init_host_contract_is_object_safe_and_fail_closeable() {
        struct MockHost;
        #[async_trait::async_trait]
        impl ProcessProvider for MockHost {
            fn run_blocking(
                &self,
                _: &crate::providers::ProcessRequest,
            ) -> io::Result<crate::providers::CapturedOutput> {
                Ok(Default::default())
            }
            async fn run_async(
                &self,
                _: &crate::providers::ProcessRequest,
            ) -> io::Result<crate::providers::CapturedOutput> {
                Ok(Default::default())
            }
            async fn spawn_piped(
                &self,
                _: &crate::providers::PipedSpawn,
            ) -> io::Result<Box<dyn crate::providers::PipedChild>> {
                Err(io::Error::other("mock"))
            }
        }
        struct NoopInit;
        impl ChildWorldInit for NoopInit {
            fn init_child(&self) -> io::Result<()> {
                Ok(())
            }
        }
        struct RefusingHost;
        #[async_trait::async_trait]
        impl ProcessProvider for RefusingHost {
            fn run_blocking(
                &self,
                _: &crate::providers::ProcessRequest,
            ) -> io::Result<crate::providers::CapturedOutput> {
                Ok(Default::default())
            }
            async fn run_async(
                &self,
                _: &crate::providers::ProcessRequest,
            ) -> io::Result<crate::providers::CapturedOutput> {
                Ok(Default::default())
            }
            async fn spawn_piped(
                &self,
                _: &crate::providers::PipedSpawn,
            ) -> io::Result<Box<dyn crate::providers::PipedChild>> {
                Err(io::Error::other("mock"))
            }
        }
        impl ForkInitHost for RefusingHost {
            fn boxed_with_fork_init(
                self: Arc<Self>,
                _init: Arc<dyn ChildWorldInit>,
            ) -> Result<Arc<dyn ProcessProvider>, String> {
                Err("remote worlds embed their own boundary".to_string())
            }
        }
        impl ForkInitHost for MockHost {
            fn boxed_with_fork_init(
                self: Arc<Self>,
                init: Arc<dyn ChildWorldInit>,
            ) -> Result<Arc<dyn ProcessProvider>, String> {
                // Wrapping keeps type erasure; tool call sites stay untouched.
                Ok(Arc::new(InitMarkingHost(init)))
            }
        }
        struct InitMarkingHost(Arc<dyn ChildWorldInit>);
        #[async_trait::async_trait]
        impl ProcessProvider for InitMarkingHost {
            fn run_blocking(
                &self,
                _: &crate::providers::ProcessRequest,
            ) -> io::Result<crate::providers::CapturedOutput> {
                self.0.init_child()?;
                Ok(crate::providers::CapturedOutput {
                    stdout: b"marked".to_vec(),
                    stderr: Vec::new(),
                    exit: crate::providers::ProcessExit::from_code(0),
                })
            }
            async fn run_async(
                &self,
                _: &crate::providers::ProcessRequest,
            ) -> io::Result<crate::providers::CapturedOutput> {
                self.0.init_child()?;
                Ok(Default::default())
            }
            async fn spawn_piped(
                &self,
                _: &crate::providers::PipedSpawn,
            ) -> io::Result<Box<dyn crate::providers::PipedChild>> {
                Err(io::Error::other("mock"))
            }
        }

        let host: Arc<dyn ForkInitHost> = Arc::new(MockHost);
        let init: Arc<dyn ChildWorldInit> = Arc::new(NoopInit);
        let decorated = host
            .clone()
            .boxed_with_fork_init(init.clone())
            .expect("host accepts init");
        let out = decorated
            .run_blocking(&crate::providers::ProcessRequest::new("true", &[]))
            .expect("run after decoration");
        assert!(out.exit.success);
        assert_eq!(out.stdout, b"marked");

        let refusing: Arc<dyn ForkInitHost> = Arc::new(RefusingHost);
        let err = match refusing.boxed_with_fork_init(init) {
            Ok(_) => panic!("refusing host must fail closed"),
            Err(e) => e,
        };
        assert!(err.contains("boundary"));
    }
}
