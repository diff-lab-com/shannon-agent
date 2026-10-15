//! Outbound-defense read-only status — the S1.6 backend ticket
//! (docs/reviews/2026-10-15-visual-convergence-r4.md §S1.6).
//!
//! One command, `get_outbound_defense_status`, exposes what the engine
//! guards actually do in this process so the Settings → 出站防线 card can
//! render live state instead of placeholder badges:
//!
//! - 出站密钥扫描: [`shannon_core::secret_guard`] — latched install mode
//!   (`redact` since v0.13.0, `audit` opt-in, `None` while never installed)
//!   plus this process's audit-hit counter;
//! - 提示注入扫描: [`shannon_core::injection_guard`] — latched mode
//!   (`audit` default, `off` opt-out) plus the same style of counter;
//! - 签名校验: a static capability note, NOT runtime state — the desktop
//!   install gate (`extensions/security.rs`) verifies skill & MCP bundles at
//!   install time only, with `SelfDeclared` semantics (the signature is
//!   parsed, never cryptographically verified). It honestly reports exactly
//!   that; there is no runtime counter to fabricate.
//!
//! Honesty contract (mirrors the UI's): read-only, zero side effects — the
//! command observes latches and counters, it never installs a guard. Fields
//! are `null` when a guard never armed; a `0` `audit_hits` means a real,
//! armed guard with no findings yet. Counters accumulate over the process
//! lifetime (a restart resets them), and in `audit` mode a hit means the
//! value was observed AND forwarded verbatim — consumers must never render
//! these counts as "blocked".

use serde::Serialize;

use shannon_core::injection_guard;
use shannon_core::secret_guard;

/// One live guard's readout: the latched mode plus the process-lifetime
/// audit-hit counter. Serialized verbatim (snake_case, lowercase mode) —
/// the UI renders these fields as-is.
#[derive(Debug, Clone, Serialize)]
pub struct SecretGuardStatus {
    /// `redact` | `audit`; the enum's lowercase `Serialize` casing.
    pub mode: secret_guard::SecretGuardMode,
    /// Secret-shaped findings audited this process (all modes; see
    /// [`secret_guard::audit_hits`]). In `audit` mode the underlying values
    /// were forwarded to the provider — never presented as "blocked".
    pub audit_hits: u64,
}

/// Same shape for the injection scanner; the mode enum differs (`audit` |
/// `off`).
#[derive(Debug, Clone, Serialize)]
pub struct InjectionGuardStatus {
    /// `audit` | `off`.
    pub mode: injection_guard::InjectionGuardMode,
    /// Injection-pattern findings audited this process (observe-only —
    /// content is never altered).
    pub audit_hits: u64,
}

/// The signature-check entry is a capability note, not a status: the check
/// runs at install time only and is self-declared (non-cryptographic). There
/// is deliberately no mode/counter here — fabricating runtime state for an
/// install-time gate would break the honesty contract.
#[derive(Debug, Clone, Serialize)]
pub struct SignatureCheckStatus {
    /// Always `true`: the only enforcement point is the install gate.
    pub install_time_only: bool,
    /// Always `"self-declared"`: signatures are parsed for a claimed signer,
    /// never verified against a public key.
    pub semantics: &'static str,
}

/// The full outbound-defense snapshot. `None` fields mean "guard never
/// installed in this process" — distinct from an armed guard at zero hits.
#[derive(Debug, Clone, Serialize)]
pub struct OutboundDefenseStatus {
    /// 出站密钥扫描 — `None` while the secret guard never installed.
    pub key_redaction: Option<SecretGuardStatus>,
    /// 提示注入扫描 — `Some` once the scanner's mode latch resolved (the
    /// first scan or status read resolves it to `audit` by default).
    pub injection_scan: Option<InjectionGuardStatus>,
    /// 签名校验 —恒为能力说明 (install gate, `SelfDeclared` semantics).
    pub signature_check: SignatureCheckStatus,
}

/// Read-only outbound-defense status for the Settings card. See the module
/// docs for the shape contract and the honesty constraints.
#[tauri::command]
pub async fn get_outbound_defense_status() -> Result<OutboundDefenseStatus, String> {
    Ok(OutboundDefenseStatus {
        key_redaction: secret_guard::enabled_mode().map(|mode| SecretGuardStatus {
            mode,
            audit_hits: secret_guard::audit_hits(),
        }),
        injection_scan: injection_guard::enabled_mode().map(|mode| InjectionGuardStatus {
            mode,
            audit_hits: injection_guard::audit_hits(),
        }),
        signature_check: SignatureCheckStatus {
            install_time_only: true,
            semantics: "self-declared",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire contract the UI renders: exact snake_case keys, lowercase
    /// modes, and `audit_hits: 0` serialized as a PRESENT zero (an armed
    /// guard with no findings), never dropped.
    #[test]
    fn serializes_snake_case_with_present_zero_counters() {
        let status = OutboundDefenseStatus {
            key_redaction: Some(SecretGuardStatus {
                mode: secret_guard::SecretGuardMode::Redact,
                audit_hits: 0,
            }),
            injection_scan: Some(InjectionGuardStatus {
                mode: injection_guard::InjectionGuardMode::Audit,
                audit_hits: 2,
            }),
            signature_check: SignatureCheckStatus {
                install_time_only: true,
                semantics: "self-declared",
            },
        };
        let v = serde_json::to_value(&status).expect("serialize");
        // Key SET is the UI contract (JSON objects are unordered; serde_json
        // without preserve_order hands them back sorted).
        let mut keys: Vec<&String> = v.as_object().expect("object").keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["injection_scan", "key_redaction", "signature_check"],
            "top-level field names are the UI contract (snake_case, exact set)"
        );
        let key = &v["key_redaction"];
        assert_eq!(key["mode"], "redact");
        assert_eq!(key["audit_hits"], 0);
        assert!(
            key.get("audit_hits").is_some(),
            "an armed guard at zero hits must serialize a PRESENT 0, not an absent field"
        );
        assert_eq!(v["injection_scan"]["mode"], "audit");
        assert_eq!(v["injection_scan"]["audit_hits"], 2);
        assert_eq!(v["signature_check"]["install_time_only"], true);
        assert_eq!(v["signature_check"]["semantics"], "self-declared");
    }

    /// Absent-vs-zero: a guard that never installed is `null`, which the UI
    /// renders as 未启用 — distinct from an armed guard at zero findings.
    #[test]
    fn unarmed_guard_serializes_null_not_zero() {
        let status = OutboundDefenseStatus {
            key_redaction: None,
            injection_scan: Some(InjectionGuardStatus {
                mode: injection_guard::InjectionGuardMode::Off,
                audit_hits: 0,
            }),
            signature_check: SignatureCheckStatus {
                install_time_only: true,
                semantics: "self-declared",
            },
        };
        let v = serde_json::to_value(&status).expect("serialize");
        assert!(
            v["key_redaction"].is_null(),
            "never-installed guard must be null"
        );
        // `off` is a real latched mode (explicit opt-out) — present, with
        // lowercase casing.
        assert_eq!(v["injection_scan"]["mode"], "off");
        assert_eq!(v["injection_scan"]["audit_hits"], 0);
    }

    /// The command itself: side-effect-free read returning the static,
    /// honest signature note and a well-formed payload for whichever guards
    /// happen to be armed in the test process (their latched state is
    /// environment-dependent and never pinned here — the struct-level tests
    /// above pin the shapes).
    #[tokio::test]
    async fn command_reports_honest_signature_note() {
        let status = get_outbound_defense_status()
            .await
            .expect("read-only status command succeeds");
        assert!(status.signature_check.install_time_only);
        assert_eq!(status.signature_check.semantics, "self-declared");
        // The readout mirrors the guards' real latches; whichever way they
        // resolved in this process, counters exist exactly when armed.
        if let Some(guard) = &status.key_redaction {
            let _ = guard.audit_hits; // u64 by type
        }
        if let Some(guard) = &status.injection_scan {
            let _ = guard.audit_hits;
        }
    }
}
