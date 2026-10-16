//! Machine-checkable dispatch semantics for every [`HookEventType`].
//!
//! This module is the **machine source of truth** for hook event dispatch
//! semantics (regime, short-circuit power, failure behavior, Claude Code
//! compatibility, producers). `docs/HOOK-SEMANTICS.md` is the human-readable
//! projection; a `#[cfg(test)]` smoke test enforces that the doc names every
//! event. The table itself is compile-checked: [`row_for`] matches on
//! [`HookEventType`] **without a wildcard arm**, so adding a variant without
//! adding its row is a compile error (P0-2 machine check).
//!
//! ## Regimes (`Regime`)
//!
//! - `EnforcedInline`: hook decisions gate the operation synchronously in the
//!   tool path (only PreToolUse, via `PreToolUseHookNode` in
//!   `shannon-core/src/query_engine/guard_nodes.rs`).
//! - `BusTriggerAdvisory`: producers publish a routing-only trigger on the
//!   session bus (`publish_hook_trigger`); the `HookManagerAdapter`
//!   subscription decodes it and runs hooks advisory (decisions are only
//!   logged as `hook/fired` audit rows).
//! - `DirectCall`: a producer constructs the `HookEvent` and calls
//!   `HookManager::run_hooks` directly (REPL / compact / coordinator /
//!   watcher / skills / instructions paths).
//! - `NoProducer`: the variant has no construction site in the repo yet.
//!
//! ## Short-circuit power (`ShortCircuit`)
//!
//! What a hook *decision* can actually do to the operation for that event:
//! `DenyModify` (deny + rewrite input — PreToolUse only), `DenyOnly`
//! (deny is consumed somewhere), `Observe` (hooks run, results/decisions are
//! dropped or only logged), `None` (no hook result is consumed at all —
//! includes all `NoProducer` events and dormant wiring).
//!
//! ## Shared failure semantics (all events, `manager.rs`)
//!
//! - Command/prompt hooks: exit 0 = parse stdout JSON for the decision
//!   (default Allow); exit 2 = `Deny` with stderr as reason; any other exit
//!   = non-blocking error (Allow). HTTP hooks: 2xx parses the body, other
//!   statuses/failures decide Deny.
//! - Blocking command/prompt/HTTP hook timeout = `Err(HookError::Timeout)`
//!   propagates out of `run_hooks`; every caller warns/logs and continues —
//!   a timed-out hook never blocks or denies (the PreToolUse node treats the
//!   error as Allow). LLM hooks are the exception: missing client, API
//!   error, or timeout decide **Deny** inside the manager.
//! - `resolve_results`: the first `Deny` short-circuits (returned
//!   immediately), the **last** `Modify` wins, otherwise `Allow`.
//! - Non-blocking (`blocking:false`) command/HTTP hooks are spawned
//!   detached, bounded by their timeout, and produce no result at all.
//!
//! ## Known gotchas (documented, intentionally unchanged here)
//!
//! - `HookConfig::if_condition` is deserialized but never evaluated
//!   (`config.rs` — `matches()` ignores it).
//! - Matcher patterns hit the **anchored regex branch first**
//!   (`^{matcher}$`), so `"Edit|Write"` acts as a regex with anchor
//!   precedence (`^Edit` or `Write$`), not as the exact pipe-split
//!   alternation that branch (only reached for invalid regexes) would give.
//! - `Stop` is always published with `should_continue: false`, so the
//!   documented exit-2-force-continue semantics are unreachable.
//! - `PostToolUseFailure` has a bus producer (P0-1) but no `decode_trigger`
//!   arm yet, so its published trigger is dropped at decode and hooks
//!   configured on it do not execute (see that row's notes).
//! - ConfigChange / InstructionsLoaded / UserPromptExpansion producer sites
//!   exist but their wiring is opt-in and unattached in production
//!   (dormant) — see those rows' notes.

use super::events::HookEventType;

/// How the event reaches `HookManager::run_hooks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regime {
    /// Enforced synchronously in the tool guard pipeline
    /// (`PreToolUseHookNode`); the only event whose decisions gate execution.
    EnforcedInline,
    /// Producer publishes a bus trigger; the `HookManagerAdapter` runs it
    /// advisory (decisions logged, never enforced).
    BusTriggerAdvisory,
    /// A producer calls `HookManager::run_hooks` directly.
    DirectCall,
    /// No construction site in the repo — enumerated for completeness / CC
    /// compatibility / future producers.
    NoProducer,
}

/// What a hook decision can actually do to the operation for this event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortCircuit {
    /// Deny blocks the call and Modify rewrites the tool input (PreToolUse).
    DenyModify,
    /// A Deny decision is consumed (operation blocked) but input is never
    /// rewritten.
    DenyOnly,
    /// Hooks run and their outcomes are logged/audited, but every decision
    /// is dropped — advisory.
    Observe,
    /// No hook result is consumed at all (no producer, or wiring never
    /// reaches `run_hooks`).
    None,
}

/// Claude Code hook-event compatibility class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcCompat {
    /// One of the 9 Claude Code standard events.
    Standard,
    /// Shannon extension beyond the CC standard set.
    Extension,
}

/// One row of the dispatch-semantics table (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEventSemantics {
    /// The event this row describes.
    pub event: HookEventType,
    /// How the event is dispatched.
    pub regime: Regime,
    /// What hook decisions can do here.
    pub short_circuit: ShortCircuit,
    /// Failure behavior summary (exit codes / timeout / resolution).
    pub failure_semantics: &'static str,
    /// Claude Code compatibility class.
    pub cc_compat: CcCompat,
    /// Producing modules (file-name paths, no line numbers); empty for
    /// `Regime::NoProducer`.
    pub producers: &'static [&'static str],
    /// Event-specific gotchas; may be empty.
    pub notes: &'static str,
}

/// Failure semantics shared by every event that reaches `run_hooks` with
/// command/HTTP/prompt hooks: the exit-code protocol plus the
/// timeout-propagates rule (callers warn and continue).
const ADVISORY_FAILURE: &str = concat!(
    "exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); ",
    "blocking timeout=Err propagates, caller warns+continues"
);

/// Resolve the table row for one event.
///
/// **Machine check**: this `match` is exhaustive by construction — no
/// wildcard arm. Adding a `HookEventType` variant without adding its row
/// here fails compilation (plan P0-2 requirement 2).
const fn row_for(event: HookEventType) -> HookEventSemantics {
    match event {
        HookEventType::PreToolUse => HookEventSemantics {
            event,
            regime: Regime::EnforcedInline,
            short_circuit: ShortCircuit::DenyModify,
            failure_semantics: concat!(
                "exit2=Deny(blocks the call); ",
                "blocking timeout=Err→node warn+Allow (a timed-out hook neither blocks nor denies); ",
                "resolve_results: first Deny short-circuits, last Modify(input) wins, else Allow"
            ),
            cc_compat: CcCompat::Standard,
            producers: &["guard_nodes.rs"],
            notes: concat!(
                "only enforced event: PreToolUseHookNode (pipeline stage `tool/guard/pre-tool-use-hooks`) ",
                "sets ctx.hook_deny and rewrites ctx.input; additionally mirrored as an advisory bus ",
                "trigger (HookManagerAdapter decodes `PreToolUse`, no engine publisher yet); ",
                "matcher regex is tried before pipe-split; if_condition parsed but never evaluated"
            ),
        },
        HookEventType::PostToolUse => HookEventSemantics {
            event,
            regime: Regime::BusTriggerAdvisory,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Standard,
            producers: &["agent_loop.rs"],
            notes: concat!(
                "trigger published by the shared post-execute helper with is_error flag; ",
                "adapter runs hooks and writes hook/fired audit rows only — decisions unused"
            ),
        },
        HookEventType::SessionStart => HookEventSemantics {
            event,
            regime: Regime::BusTriggerAdvisory,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Standard,
            producers: &["agent_loop.rs", "repl/mod.rs"],
            notes: concat!(
                "engine fires once per session at the first query; the REPL fires it at startup and ",
                "pre-marks the flag so the engine stays silent (no double-fire); ",
                "the REPL path is a direct run_hooks call whose results are dropped"
            ),
        },
        HookEventType::SessionEnd => HookEventSemantics {
            event,
            regime: Regime::BusTriggerAdvisory,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Standard,
            producers: &["repl/mod.rs"],
            notes: concat!(
                "REPL fires it directly during shutdown before the sidecar save; results dropped; ",
                "the adapter decodes this trigger type so future bus producers route advisory"
            ),
        },
        HookEventType::Notification => HookEventSemantics {
            event,
            regime: Regime::NoProducer,
            short_circuit: ShortCircuit::None,
            failure_semantics: "no producer: nothing runs",
            cc_compat: CcCompat::Standard,
            producers: &[],
            notes: "CC standard event; enumerated but never constructed in-repo",
        },
        HookEventType::UserPromptSubmit => HookEventSemantics {
            event,
            regime: Regime::BusTriggerAdvisory,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Standard,
            producers: &["agent_loop.rs"],
            notes: concat!(
                "fired per user message right after the L0 turn start; ",
                "results unused (CC's stdout-fed-back-to-model behavior is not implemented)"
            ),
        },
        HookEventType::TeamTaskCreated => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Extension,
            producers: &["coordinator.rs"],
            notes: concat!(
                "coordinator fire_hook (spawned, non-blocking); variant doc claims exit2=rollback ",
                "but fire_hook only logs it — not enforced"
            ),
        },
        HookEventType::TeamTaskCompleted => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Extension,
            producers: &["coordinator.rs"],
            notes: concat!(
                "coordinator fire_hook (spawned, non-blocking); variant doc claims exit2=prevent ",
                "completion but fire_hook only logs it — not enforced"
            ),
        },
        HookEventType::TeammateIdle => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Extension,
            producers: &["coordinator.rs"],
            notes: concat!(
                "coordinator fire_hook (spawned, non-blocking); variant doc claims exit2=feedback+keep ",
                "working but fire_hook only logs it — not enforced"
            ),
        },
        HookEventType::PreCompact => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Standard,
            producers: &["compact/engine.rs"],
            notes: concat!(
                "fired on a spawned task before compaction — completion is NOT awaited, so it can ",
                "race the compaction it precedes; compact proceeds regardless"
            ),
        },
        HookEventType::SubagentStart => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Extension,
            producers: &["coordinator.rs"],
            notes: "spawned fire-and-forget before teammate work starts",
        },
        HookEventType::SubagentStop => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Standard,
            producers: &["coordinator.rs"],
            notes: "spawned fire-and-forget on the success/timeout/failure paths with result_summary",
        },
        HookEventType::PermissionDenied => HookEventSemantics {
            event,
            regime: Regime::NoProducer,
            short_circuit: ShortCircuit::None,
            failure_semantics: "no producer: nothing runs",
            cc_compat: CcCompat::Extension,
            producers: &[],
            notes: "",
        },
        HookEventType::Stop => HookEventSemantics {
            event,
            regime: Regime::BusTriggerAdvisory,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Standard,
            producers: &["agent_loop.rs"],
            notes: concat!(
                "published immediately before QueryEvent::Completed; should_continue is hardcoded ",
                "false, so the documented exit-2-force-continue semantics are unreachable"
            ),
        },
        HookEventType::PostToolUseFailure => HookEventSemantics {
            event,
            regime: Regime::BusTriggerAdvisory,
            short_circuit: ShortCircuit::None,
            failure_semantics: concat!(
                "producer publishes the trigger, but decode_trigger has no arm for this type yet — ",
                "the trigger is dropped at decode and run_hooks never executes"
            ),
            cc_compat: CcCompat::Extension,
            producers: &["agent_loop.rs"],
            notes: concat!(
                "P0-1 gave this event its first producer (tool Err path); the payload mirrors the ",
                "PostToolUse shape (output + is_error=true), not the variant's `error` field, and ",
                "`decode_trigger` (guard_nodes.rs) cannot decode it yet — hooks configured on this ",
                "event do not execute; no hook result is ever consumed (hence None)"
            ),
        },
        HookEventType::PostCompact => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Extension,
            producers: &["compact/engine.rs"],
            notes: "spawned fire-and-forget after successful compaction (before/after counts + tokens_freed)",
        },
        HookEventType::StopFailure => HookEventSemantics {
            event,
            regime: Regime::NoProducer,
            short_circuit: ShortCircuit::None,
            failure_semantics: "no producer: nothing runs",
            cc_compat: CcCompat::Extension,
            producers: &[],
            notes: "",
        },
        HookEventType::FileChanged => HookEventSemantics {
            event,
            regime: Regime::NoProducer,
            short_circuit: ShortCircuit::None,
            failure_semantics: "no producer: nothing runs",
            cc_compat: CcCompat::Extension,
            producers: &[],
            notes: "",
        },
        HookEventType::CwdChanged => HookEventSemantics {
            event,
            regime: Regime::NoProducer,
            short_circuit: ShortCircuit::None,
            failure_semantics: "no producer: nothing runs",
            cc_compat: CcCompat::Extension,
            producers: &[],
            notes: "",
        },
        HookEventType::PermissionRequest => HookEventSemantics {
            event,
            regime: Regime::NoProducer,
            short_circuit: ShortCircuit::None,
            failure_semantics: "no producer: nothing runs",
            cc_compat: CcCompat::Extension,
            producers: &[],
            notes: "",
        },
        HookEventType::UserPromptExpansion => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::None,
            failure_semantics: concat!(
                "dormant wiring: no hook result is ever consumed while no emitter is attached"
            ),
            cc_compat: CcCompat::Extension,
            producers: &["skills/executor.rs"],
            notes: concat!(
                "executor emits the serialized event to an optional HookEmitter after template ",
                "substitution; no production host attaches one, so run_hooks is never reached"
            ),
        },
        HookEventType::PostToolBatch => HookEventSemantics {
            event,
            regime: Regime::NoProducer,
            short_circuit: ShortCircuit::None,
            failure_semantics: "no producer: nothing runs",
            cc_compat: CcCompat::Extension,
            producers: &[],
            notes: "",
        },
        HookEventType::ConfigChange => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::None,
            failure_semantics: concat!(
                "dormant wiring: no hook result is ever consumed while no callback is attached"
            ),
            cc_compat: CcCompat::Extension,
            producers: &["config_watcher.rs"],
            notes: concat!(
                "watcher converts debounced detections via to_hook_event for a run_hooks-style ",
                "callback; `watch_local_toml` has no production caller — dormant"
            ),
        },
        HookEventType::InstructionsLoaded => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::None,
            failure_semantics: concat!(
                "dormant wiring: no hook result is ever consumed while no emitter is installed"
            ),
            cc_compat: CcCompat::Extension,
            producers: &["project_instructions.rs"],
            notes: concat!(
                "global INSTRUCTION_HOOK_EMITTER (OnceLock) fires after the instruction merge; ",
                "the default is a no-op and only tests install an emitter — dormant in production"
            ),
        },
        HookEventType::WorktreeCreate => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Extension,
            producers: &["coordinator.rs"],
            notes: "coordinator fire_hook (spawned, non-blocking); exit 2 logged only",
        },
        HookEventType::WorktreeRemove => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Extension,
            producers: &["coordinator.rs"],
            notes: "coordinator fire_hook (spawned, non-blocking); exit 2 logged only",
        },
        HookEventType::Elicitation => HookEventSemantics {
            event,
            regime: Regime::NoProducer,
            short_circuit: ShortCircuit::None,
            failure_semantics: "no producer: nothing runs",
            cc_compat: CcCompat::Extension,
            producers: &[],
            notes: "",
        },
        HookEventType::ElicitationResult => HookEventSemantics {
            event,
            regime: Regime::NoProducer,
            short_circuit: ShortCircuit::None,
            failure_semantics: "no producer: nothing runs",
            cc_compat: CcCompat::Extension,
            producers: &[],
            notes: "",
        },
        HookEventType::TaskCreated => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Extension,
            producers: &["coordinator.rs"],
            notes: concat!(
                "events.rs doc calls this a CC standard name, but it is classified Extension per ",
                "the plan's fixed 9-event CC set"
            ),
        },
        HookEventType::TaskCompleted => HookEventSemantics {
            event,
            regime: Regime::DirectCall,
            short_circuit: ShortCircuit::Observe,
            failure_semantics: ADVISORY_FAILURE,
            cc_compat: CcCompat::Extension,
            producers: &["coordinator.rs"],
            notes: concat!(
                "events.rs doc calls this a CC standard name, but it is classified Extension per ",
                "the plan's fixed 9-event CC set"
            ),
        },
    }
}

/// The full dispatch-semantics table: exactly one row per `HookEventType`
/// variant, in enum declaration order.
///
/// Rows come from [`row_for`]'s exhaustive match — a new enum variant
/// without a row is a **compile error**, which is the CI machine check.
pub fn all_event_semantics() -> &'static [HookEventSemantics] {
    const TABLE: &[HookEventSemantics] = &[
        row_for(HookEventType::PreToolUse),
        row_for(HookEventType::PostToolUse),
        row_for(HookEventType::SessionStart),
        row_for(HookEventType::SessionEnd),
        row_for(HookEventType::Notification),
        row_for(HookEventType::UserPromptSubmit),
        row_for(HookEventType::TeamTaskCreated),
        row_for(HookEventType::TeamTaskCompleted),
        row_for(HookEventType::TeammateIdle),
        row_for(HookEventType::PreCompact),
        row_for(HookEventType::SubagentStart),
        row_for(HookEventType::SubagentStop),
        row_for(HookEventType::PermissionDenied),
        row_for(HookEventType::Stop),
        row_for(HookEventType::PostToolUseFailure),
        row_for(HookEventType::PostCompact),
        row_for(HookEventType::StopFailure),
        row_for(HookEventType::FileChanged),
        row_for(HookEventType::CwdChanged),
        row_for(HookEventType::PermissionRequest),
        row_for(HookEventType::UserPromptExpansion),
        row_for(HookEventType::PostToolBatch),
        row_for(HookEventType::ConfigChange),
        row_for(HookEventType::InstructionsLoaded),
        row_for(HookEventType::WorktreeCreate),
        row_for(HookEventType::WorktreeRemove),
        row_for(HookEventType::Elicitation),
        row_for(HookEventType::ElicitationResult),
        row_for(HookEventType::TaskCreated),
        row_for(HookEventType::TaskCompleted),
    ];
    TABLE
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Names of the 9 Claude Code standard events (plan P0-2 grounding).
    const CC_STANDARD_9: [&str; 9] = [
        "PreToolUse",
        "PostToolUse",
        "SessionStart",
        "SessionEnd",
        "Notification",
        "UserPromptSubmit",
        "Stop",
        "SubagentStop",
        "PreCompact",
    ];

    #[test]
    fn table_has_exactly_thirty_rows() {
        assert_eq!(all_event_semantics().len(), 30);
    }

    #[test]
    fn table_has_no_duplicate_events() {
        let events: HashSet<&HookEventType> =
            all_event_semantics().iter().map(|r| &r.event).collect();
        assert_eq!(
            events.len(),
            all_event_semantics().len(),
            "duplicate event rows in the semantics table"
        );
    }

    #[test]
    fn every_row_round_trips_through_the_wire_name() {
        // Each row's event must survive name routing: Display →
        // from_str_lossy → same variant. This is what hook configs key on.
        for row in all_event_semantics() {
            let name = row.event.to_string();
            assert_eq!(
                HookEventType::from_str_lossy(&name),
                Some(row.event.clone()),
                "wire name round-trip failed for {name}"
            );
        }
    }

    #[test]
    fn no_producer_rows_are_exactly_the_nine_variants() {
        let no_producer: Vec<String> = all_event_semantics()
            .iter()
            .filter(|r| r.regime == Regime::NoProducer)
            .map(|r| r.event.to_string())
            .collect();
        let expected: &[&str] = &[
            "StopFailure",
            "Notification",
            "PermissionDenied",
            "PermissionRequest",
            "FileChanged",
            "CwdChanged",
            "PostToolBatch",
            "Elicitation",
            "ElicitationResult",
        ];
        let mut sorted = no_producer.clone();
        sorted.sort();
        let mut expected_sorted: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        expected_sorted.sort();
        assert_eq!(sorted, expected_sorted, "NoProducer group drifted");
        // PostToolUseFailure left the group in P0-1 (first producer landed).
        assert!(!no_producer.iter().any(|s| s == "PostToolUseFailure"));
    }

    #[test]
    fn producer_invariants_hold_per_regime() {
        for row in all_event_semantics() {
            match row.regime {
                Regime::NoProducer => {
                    assert!(
                        row.producers.is_empty(),
                        "{}: NoProducer rows must have no producers",
                        row.event
                    );
                    assert_eq!(
                        row.short_circuit,
                        ShortCircuit::None,
                        "{}: NoProducer rows consume no hook result",
                        row.event
                    );
                }
                _ => {
                    assert!(
                        !row.producers.is_empty(),
                        "{}: non-NoProducer rows must name producers",
                        row.event
                    );
                }
            }
        }
    }

    #[test]
    fn cc_standard_is_exactly_the_nine_event_set() {
        let standard: Vec<String> = all_event_semantics()
            .iter()
            .filter(|r| r.cc_compat == CcCompat::Standard)
            .map(|r| r.event.to_string())
            .collect();
        let mut sorted = standard.clone();
        sorted.sort();
        let mut expected = CC_STANDARD_9.to_vec();
        expected.sort();
        assert_eq!(
            sorted, expected,
            "CC Standard set drifted from the 9-event set"
        );
        assert_eq!(standard.len(), 9);
    }

    #[test]
    fn enforced_inline_events_route_through_manager_config() {
        // Engine-local routing capability for the enforced event(s): the
        // wire name must address a HooksFile entry and match by subject —
        // the same routing `PreToolUseHookNode` exercises. (The bus
        // Custom-kind subscription capability for all 30 names is proven
        // cross-crate in shannon-core guard_nodes.rs tests; asserting it
        // here would need a reverse shannon-core dependency.)
        for row in all_event_semantics() {
            if row.regime != Regime::EnforcedInline {
                continue;
            }
            let name = row.event.to_string();
            let mut hooks_file = super::super::config::HooksFile::new();
            hooks_file.hooks.insert(
                name.clone(),
                vec![super::super::config::HookConfig::new("Bash")],
            );
            let configs = hooks_file.get_for_event(&row.event);
            assert_eq!(configs.len(), 1, "{name}: event name must route by key");
            assert!(
                configs[0].matches("Bash"),
                "{name}: matcher must apply to the tool-name subject"
            );
        }
        // Sanity: PreToolUse is the single enforced-inline event today.
        let enforced: Vec<String> = all_event_semantics()
            .iter()
            .filter(|r| r.regime == Regime::EnforcedInline)
            .map(|r| r.event.to_string())
            .collect();
        assert_eq!(enforced, vec!["PreToolUse".to_string()]);
    }

    #[test]
    fn hook_semantics_doc_names_every_event() {
        // Smoke test (plan P0-2 requirement 4): docs/HOOK-SEMANTICS.md must
        // exist and mention every one of the 30 event names. No per-line
        // diff — presence is the contract; `docs/plans/…` owns the prose.
        let doc_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/HOOK-SEMANTICS.md");
        let content = std::fs::read_to_string(&doc_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", doc_path.display()));
        for row in all_event_semantics() {
            let name = row.event.to_string();
            assert!(
                content.contains(&name),
                "docs/HOOK-SEMANTICS.md is missing event `{name}`"
            );
            // Cell-delimited match so e.g. `Stop` cannot be satisfied by
            // the `StopFailure` row alone.
            let cell = format!("| {name} |");
            assert!(
                content.contains(&cell),
                "docs/HOOK-SEMANTICS.md has no table cell for `{name}`"
            );
        }
        // The doc must name the machine source of truth.
        assert!(
            content.contains("semantics.rs"),
            "doc header must point at semantics.rs as the machine source of truth"
        );
    }
}
