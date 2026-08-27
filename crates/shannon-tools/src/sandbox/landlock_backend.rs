//! Linux Landlock backend (§4.12 W3-3b).
//!
//! Kernel-enforced execution world: every forked child receives a Landlock
//! ruleset between fork and exec via the [`ForkInitHost`] seam, so bash/LSP
//! (and anything else riding the process world) runs restricted without a
//! single line of tool-code change.
//!
//! ## Enforcement model
//!
//! - The parent process stays unrestricted on purpose (session log writes,
//!   LLM traffic, telemetry all keep working); restriction is per-child.
//! - Rulesets rebuild per spawn from the resolved grant table. A failing
//!   install aborts the spawn — a child can never start without its boundary
//!   — and every failure carries the [`SANDBOX_DENIED_PREFIX`] marker so it
//!   classifies as `sandbox_denied` downstream.
//! - `NotEnforced` status refuses execution explicitly, making silent fake
//!   sandboxes impossible on seccomp-filtered hosts.
//! - ABI selection happens once at construction by probing rule creation
//!   (`HardRequirement`): the chosen level is fully supported, so no granted
//!   access bit is silently dropped later.
//!
//! ## Non-Linux builds
//!
//! A stub keeps [`probe_new`] available on all platforms; it fails closed
//! with [`SandboxError::Unsupported`] (surfaced loudly by callers).

use shannon_tool_interface::sandbox::{
    ChildWorldInit, DegradeNotice, SANDBOX_DENIED_PREFIX, SandboxError, SandboxPolicy,
    SandboxProvider,
};
use shannon_tool_interface::{FileSystemProvider, ForkInitHost, ProcessProvider};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::SandboxedFs;
use shannon_tool_interface::sandbox::path_within;

// ---------------------------------------------------------------------------
// Landlock access-bit vocabulary (kernel-numbered, platform-portable)
//
// Keeping a plain-u64 mirror of the kernel's `landlock_access_fs` bits lets
// the grant-table math compile and unit-test on every OS; the numbers are the
// canonical ordering from include/uapi/linux/landlock.h.
// ---------------------------------------------------------------------------

/// `LANDLOCK_ACCESS_FS_EXECUTE`
pub const FS_EXECUTE: u64 = 1 << 0;
/// `LANDLOCK_ACCESS_FS_WRITE_FILE`
pub const FS_WRITE_FILE: u64 = 1 << 1;
/// `LANDLOCK_ACCESS_FS_READ_FILE`
pub const FS_READ_FILE: u64 = 1 << 2;
/// `LANDLOCK_ACCESS_FS_READ_DIR`
pub const FS_READ_DIR: u64 = 1 << 3;
/// `LANDLOCK_ACCESS_FS_REMOVE_DIR`
pub const FS_REMOVE_DIR: u64 = 1 << 4;
/// `LANDLOCK_ACCESS_FS_REMOVE_FILE`
pub const FS_REMOVE_FILE: u64 = 1 << 5;
/// All `MAKE_*` rights (char/dir/reg/sock/fifo/block/sym), ABI v1 as a group.
const FS_MAKE_ALL: u64 =
    (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 10) | (1 << 11) | (1 << 12);
/// `LANDLOCK_ACCESS_FS_REFER` (ABI v2)
const FS_REFER: u64 = 1 << 13;
/// `LANDLOCK_ACCESS_FS_TRUNCATE` (ABI v3)
const FS_TRUNCATE: u64 = 1 << 14;

/// Read rights beneath every root. Deliberately **without** `FS_EXECUTE`:
/// executability is a separate capability (writable roots + the
/// `executable_roots` allow-list) — otherwise reading `/` would make every
/// binary runnable and the exec test-granularity would vanish.
const READ_SET: u64 = FS_READ_FILE | FS_READ_DIR;

/// Write rights by Landlock ABI version (cumulative).
fn write_set_for_level(level: u32) -> u64 {
    let base = FS_WRITE_FILE | FS_REMOVE_DIR | FS_REMOVE_FILE | FS_MAKE_ALL;
    match level {
        0..=1 => base,
        2 => base | FS_REFER,
        _ => base | FS_REFER | FS_TRUNCATE,
    }
}

/// One resolved allow-list entry: absolute root plus its combined access mask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// Canonicalized absolute root directory.
    pub path: PathBuf,
    /// Union of [`FS_*`] access bits granted beneath `path`.
    pub access: u64,
}

/// Pure policy → allow-list resolution (unit-testable on any host).
///
/// Writable roots dominate: they receive full read+write. Readable-only roots
/// get read. Executable roots contribute execute+read when not already wider.
/// Overlaps merge by widening.
pub fn resolve_grants(policy: &SandboxPolicy, abi_level: u32) -> Vec<Grant> {
    let write_all = write_set_for_level(abi_level);
    let mut merged: Vec<Grant> = Vec::new();

    let widen = |merged: &mut Vec<Grant>, root: &Path, access: u64| {
        if let Some(existing) = merged.iter_mut().find(|g| g.path == root) {
            existing.access |= access;
        } else {
            merged.push(Grant {
                path: root.to_path_buf(),
                access,
            });
        }
    };

    for root in &policy.writable_roots {
        // Writable implies readable + executable (built artifacts run).
        widen(&mut merged, root, READ_SET | write_all | FS_EXECUTE);
    }
    for root in &policy.executable_roots {
        widen(&mut merged, root, READ_SET | FS_EXECUTE);
    }
    for root in &policy.readable_roots {
        widen(&mut merged, root, READ_SET);
    }

    // Roots absorbed by a strictly-wider containing ancestor collapse onto
    // it (equal-path entries were already widened together above).
    let keep: Vec<bool> = (0..merged.len())
        .map(|i| {
            let g = &merged[i];
            !merged.iter().any(|o| {
                o.path != g.path
                    && path_within(&g.path, &o.path)
                    && (o.access & g.access) == g.access
            })
        })
        .collect();
    let mut index = 0usize;
    merged.retain(|_| {
        let k = keep[index];
        index += 1;
        k
    });
    merged.sort_by(|a, b| a.path.cmp(&b.path));
    merged
}

/// Highest ABI the running kernel supports, expressed back as a name +
/// level pair; error when Landlock itself is missing (pre-5.13 kernels).
///
/// Probes use ruleset **creation** only — never restricts this thread.
#[cfg(target_os = "linux")]
fn probe_abi() -> Result<(&'static str, u32), String> {
    use landlock::{ABI, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr};

    const CANDIDATES: [(&str, u32); 5] = [("v5", 5), ("v4", 4), ("v3", 3), ("v2", 2), ("v1", 1)];

    let abi_of = |level: u32| match level {
        5 => ABI::V5,
        4 => ABI::V4,
        3 => ABI::V3,
        2 => ABI::V2,
        _ => ABI::V1,
    };

    let mut last_err = "Landlock syscalls unavailable".to_string();
    for (name, level) in CANDIDATES.iter() {
        let handled = AccessFs::from_read(abi_of(*level)) | AccessFs::from_write(abi_of(*level));
        let attempt = Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(handled)
            .and_then(|rs| rs.create());
        match attempt {
            Ok(_created) => return Ok((name, *level)),
            Err(e) => last_err = format!("ABI {name}: {e}"),
        }
    }
    Err(format!(
        "kernel lacks Landlock support (requires Linux >= 5.13): {last_err}"
    ))
}

/// Does the running kernel support the v4 network ABI?
#[cfg(target_os = "linux")]
fn probe_net_abi() -> bool {
    use landlock::{AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr};
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessNet::BindTcp | AccessNet::ConnectTcp)
        .and_then(|rs| rs.create())
        .is_ok()
}

/// Parse compiled-but-stubbed hosts gracefully in shared code paths.
fn hard_unsupported(detail: String) -> SandboxError {
    SandboxError::Unsupported {
        backend: "landlock".to_string(),
        detail,
    }
}

// ---------------------------------------------------------------------------
// Fork-time installer
// ---------------------------------------------------------------------------

/// Child-world initializer snapshot handed to every fork.
struct WorldInstall {
    grants: Arc<Vec<Grant>>,
    net_denied: bool,
}

impl WorldInstall {
    /// Build a fresh ruleset and restrict the caller (the forked child).
    #[cfg(target_os = "linux")]
    fn restrict_this_child(grants: &[Grant], net_denied: bool) -> io::Result<()> {
        use landlock::{
            AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
            RulesetAttr, RulesetCreatedAttr, RulesetStatus,
        };

        const BIT_TABLE: &[(u64, AccessFs)] = &[
            (FS_EXECUTE, AccessFs::Execute),
            (FS_WRITE_FILE, AccessFs::WriteFile),
            (FS_READ_FILE, AccessFs::ReadFile),
            (FS_READ_DIR, AccessFs::ReadDir),
            (FS_REMOVE_DIR, AccessFs::RemoveDir),
            (FS_REMOVE_FILE, AccessFs::RemoveFile),
            (1 << 6, AccessFs::MakeChar),
            (1 << 7, AccessFs::MakeDir),
            (1 << 8, AccessFs::MakeReg),
            (1 << 9, AccessFs::MakeSock),
            (1 << 10, AccessFs::MakeFifo),
            (1 << 11, AccessFs::MakeBlock),
            (1 << 12, AccessFs::MakeSym),
            (FS_REFER, AccessFs::Refer),
            (FS_TRUNCATE, AccessFs::Truncate),
        ];

        /// Wrap a Landlock ruleset error into the canonical denial form.
        fn deny(what: &str, e: landlock::RulesetError) -> io::Error {
            io::Error::other(format!(
                "{SANDBOX_DENIED_PREFIX}: ruleset install failed ({what}): {e}"
            ))
        }
        let deny_plain = |msg: String| -> io::Error {
            io::Error::other(format!("{SANDBOX_DENIED_PREFIX}: {msg}"))
        };

        // Builder-style consumption: each step takes `self` by value, so
        // rebind through the conditional chain.
        let base = Ruleset::default().set_compatibility(CompatLevel::HardRequirement);

        let mut handled_fs: BitFlags<AccessFs> = Default::default();
        for grant in grants {
            for (bit, access) in BIT_TABLE {
                if grant.access & bit != 0 {
                    handled_fs |= *access;
                }
            }
        }
        let base = if !handled_fs.is_empty() {
            base.handle_access(handled_fs)
                .map_err(|e| deny("handle_fs", e))?
        } else {
            base
        };
        let base = if net_denied {
            base.handle_access(AccessNet::BindTcp | AccessNet::ConnectTcp)
                .map_err(|e| deny("handle_net", e))?
        } else {
            base
        };
        let created = base.create().map_err(|e| deny("create", e))?;

        let mut prepared = created;
        for grant in grants {
            let fd = PathFd::new(&grant.path).map_err(|e| {
                deny_plain(format!(
                    "root {} unreadable at fork: {e}",
                    grant.path.display()
                ))
            })?;
            let mut access: BitFlags<AccessFs> = Default::default();
            for (bit, right) in BIT_TABLE {
                if grant.access & bit != 0 {
                    access |= *right;
                }
            }
            if !access.is_empty() {
                prepared = prepared
                    .add_rule(PathBeneath::new(fd, access))
                    .map_err(|e| deny("add_rule", e))?;
            }
        }
        // net_denied adds zero NetPort rules ⇒ every bind/connect denied.

        let status = prepared
            .restrict_self()
            .map_err(|e| deny("restrict_self", e))?;
        if status.ruleset == RulesetStatus::NotEnforced {
            return Err(deny_plain(
                "kernel reported NotEnforced; refusing to exec without boundary".to_string(),
            ));
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn restrict_this_child(_grants: &[Grant], _net_denied: bool) -> io::Result<()> {
        Err(io::Error::other(format!(
            "{SANDBOX_DENIED_PREFIX}: this build lacks the Linux Landlock backend"
        )))
    }
}

impl ChildWorldInit for WorldInstall {
    fn init_child(&self) -> io::Result<()> {
        Self::restrict_this_child(&self.grants, self.net_denied)
    }
}

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod imp {
    use super::*;

    /// Kernel-enforced sandbox backend (Linux).
    pub struct Backend {
        policy: Arc<SandboxPolicy>,
        abi_name: &'static str,
        grants: Arc<Vec<Grant>>,
        net_denied: bool,
        notices: Vec<DegradeNotice>,
    }

    impl Backend {
        /// Probed Landlock ABI the running kernel enforces (`"v1"`.."v5");
        /// surfaced so operators can confirm full-ABI enforcement (e.g.
        /// network gating needs v4).
        pub fn abi(&self) -> &'static str {
            self.abi_name
        }

        pub(super) fn new(policy: Arc<SandboxPolicy>) -> Result<Self, SandboxError> {
            let (abi_name, level) = probe_abi().map_err(hard_unsupported)?;
            tracing::info!(abi = abi_name, "sandbox: landlock backend ready");

            let mut notices = Vec::new();
            let mut net_denied = false;
            if !policy.network {
                if probe_net_abi() {
                    net_denied = true;
                } else {
                    notices.push(DegradeNotice::new(
                        "net-abi-unavailable",
                        "kernel lacks the Landlock network ABI (>= 6.7 / v4); children keep \
                         normal host network permissions — combine with a firewall profile for \
                         full network isolation",
                    ));
                }
            }

            // Open-check each configured root; unopenable paths shrink the
            // allow-list (fail-closed direction) and surface as loud notices.
            let mut grants = Vec::new();
            for candidate in resolve_grants(&policy, level) {
                if std::fs::metadata(&candidate.path).is_err() {
                    notices.push(DegradeNotice::new(
                        "path-unreadable",
                        format!(
                            "{}: configured sandbox root cannot be opened; it will NOT be \
                             granted (children lose access to it)",
                            candidate.path.display()
                        ),
                    ));
                    continue;
                }
                grants.push(candidate);
            }
            if grants.is_empty() {
                return Err(SandboxError::InvalidConfig(
                    "landlock policy resolves to zero usable grants; refusing an unusable world"
                        .to_string(),
                ));
            }

            Ok(Self {
                policy,
                abi_name,
                grants: Arc::new(grants),
                net_denied,
                notices,
            })
        }
    }

    impl SandboxProvider for Backend {
        fn kind(&self) -> &'static str {
            "landlock"
        }

        fn policy(&self) -> &SandboxPolicy {
            &self.policy
        }

        fn degrade_notices(&self) -> &[DegradeNotice] {
            &self.notices
        }

        fn decorate_fs(&self, inner: Arc<dyn FileSystemProvider>) -> Arc<dyn FileSystemProvider> {
            Arc::new(SandboxedFs::new(inner, self.policy.clone()))
        }

        fn decorate_process(
            &self,
            inner: Arc<dyn ForkInitHost>,
        ) -> Result<Arc<dyn ProcessProvider>, SandboxError> {
            let init = Arc::new(WorldInstall {
                grants: self.grants.clone(),
                net_denied: self.net_denied,
            });
            let decorated = inner.boxed_with_fork_init(init).map_err(|e| {
                SandboxError::Undecorable(format!(
                    "process world cannot host fork enforcement: {e}"
                ))
            })?;
            Ok(Arc::new(super::super::SandboxedProcess::new(
                decorated,
                "landlock",
                vec![
                    ("LC_ALL".to_string(), "C".to_string()),
                    ("LANG".to_string(), "C".to_string()),
                ],
            )))
        }
    }
}

#[cfg(target_os = "linux")]
pub use imp::Backend as LandlockBackend;

/// Probe the host and return a ready-to-decorate backend.
///
/// On non-Linux builds this is always [`SandboxError::Unsupported`]; the
/// assembly layer surfaces it verbatim (fail-closed, never silently local).
pub fn probe_new(policy: Arc<SandboxPolicy>) -> Result<Arc<dyn SandboxProvider>, SandboxError> {
    #[cfg(target_os = "linux")]
    {
        Ok(Arc::new(LandlockBackend::new(policy)?))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = policy;
        Err(hard_unsupported(
            "this build does not include Linux Landlock support (target_os != linux)".to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn policy(network: bool) -> SandboxPolicy {
        SandboxPolicy {
            writable_roots: vec![PathBuf::from("/proj")],
            readable_roots: vec![PathBuf::from("/")],
            executable_roots: vec![PathBuf::from("/usr")],
            network,
        }
    }

    #[test]
    fn grant_resolution_writes_dominate_reads() {
        let grants = resolve_grants(&policy(false), 3);
        let proj = grants
            .iter()
            .find(|g| g.path == Path::new("/proj"))
            .expect("proj");
        let root = grants
            .iter()
            .find(|g| g.path == Path::new("/"))
            .expect("root");
        assert_eq!(proj.access & FS_WRITE_FILE, FS_WRITE_FILE);
        assert_eq!(root.access & FS_WRITE_FILE, 0, "/ is read-only");
        assert_eq!(root.access & FS_READ_DIR, FS_READ_DIR);
    }

    #[test]
    fn grant_resolution_is_abi_aware_and_overlapping_merge_widens() {
        let proj = Path::new("/proj");
        // TRUNCATE exists only from ABI v3.
        let v2 = resolve_grants(&policy(false), 2);
        let v3 = resolve_grants(&policy(false), 3);
        let p2 = v2.iter().find(|g| g.path == proj).expect("proj");
        let p3 = v3.iter().find(|g| g.path == proj).expect("proj");
        assert_eq!(p2.access & FS_TRUNCATE, 0);
        assert_eq!(p3.access & FS_TRUNCATE, FS_TRUNCATE);

        let overlapping = SandboxPolicy {
            writable_roots: vec![PathBuf::from("/")],
            readable_roots: vec![PathBuf::from("/")],
            executable_roots: vec![Path::new("/usr").to_path_buf()],
            network: false,
        };
        let grants = resolve_grants(&overlapping, 3);
        let root = grants.iter().find(|g| g.path == Path::new("/")).expect("/");
        assert_eq!(root.access & FS_WRITE_FILE, FS_WRITE_FILE, "wider wins");
        // The absorbed /usr entry collapses into the / ancestor.
        assert!(
            !grants.iter().any(|g| g.path == Path::new("/usr")),
            "fully-contained narrower entries must merge away"
        );
    }

    /// Grant tables never silently explode their width beyond configured
    /// tiers: a fresh default policy must not grant writes or executability
    /// through plain readability of "/".
    #[test]
    fn readability_never_implies_writability_or_executability() {
        let grants = resolve_grants(&policy(false), 5);
        for grant in &grants {
            if grant.path == Path::new("/") {
                assert_eq!(grant.access & FS_MAKE_ALL, 0);
                assert_eq!(grant.access & FS_TRUNCATE, 0);
                assert_eq!(grant.access & FS_WRITE_FILE, 0);
                assert_eq!(
                    grant.access & FS_EXECUTE,
                    0,
                    "read-only roots must not silently allow execution"
                );
            }
        }
        let proj = grants
            .iter()
            .find(|g| g.path == Path::new("/proj"))
            .expect("proj");
        assert_eq!(
            proj.access & FS_EXECUTE,
            FS_EXECUTE,
            "writable roots run artifacts"
        );
    }

    /// Stub hosts refuse construction instead of faking a sandbox.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_probe_fails_closed() {
        let err = probe_new(Arc::new(policy(false))).expect_err("must fail closed");
        assert!(matches!(err, SandboxError::Unsupported { .. }));
    }

    /// Real-host behavior: either the machine supports Landlock (backend
    /// comes up with at least the workspace grant and correct identity) or
    /// the probe reports Unsupported — both accepted, nothing silent.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_probe_matches_kernel_capability() {
        let result = probe_new(Arc::new(policy(false)));
        match result {
            Ok(backend) => {
                assert_eq!(backend.kind(), "landlock");
                assert!(!backend.policy().network);
                let major =
                    std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
                println!("host kernel: {} (landlock active)", major.trim());
            }
            Err(SandboxError::Unsupported { detail, .. }) => {
                println!("host lacks landlock: {detail}");
            }
            Err(other) => panic!("unexpected sandbox error: {other}"),
        }
    }
}
