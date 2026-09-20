//! Query complexity routing heuristics (fast/plan/primary model selection).
//!
//! Split out of `engine.rs` (Wave 2 architecture step). NOTE: still a pure
//! keyword/length heuristic — a placeholder for semantic routing.

// ── Query complexity classification ──────────────────────────────────

/// Query complexity level for model routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueryComplexity {
    /// Simple lookup, short question — route to fast_model
    Simple,
    /// Planning, architecture, design — route to plan_model
    Planning,
    /// Standard coding task — use primary model
    Standard,
}

/// Keywords that signal planning/architecture queries.
const PLANNING_KEYWORDS: &[&str] = &[
    "architect",
    "architecture",
    "design",
    "plan",
    "planning",
    "refactor",
    "migrate",
    "strategy",
    "blueprint",
    "roadmap",
    "system design",
    "evaluate",
    "analyze",
    "review",
];

/// Keywords that signal complex implementation queries.
const COMPLEX_KEYWORDS: &[&str] = &[
    "implement",
    "build",
    "create",
    "develop",
    "integrate",
    "debug",
    "fix",
    "solve",
    "troubleshoot",
];

/// Classify a user query by complexity for model routing.
pub(super) fn classify_query_complexity(query: &str) -> QueryComplexity {
    let lower = query.to_lowercase();

    // Short queries with no complex keywords → Simple
    if query.len() < 200
        && !PLANNING_KEYWORDS.iter().any(|k| lower.contains(k))
        && !COMPLEX_KEYWORDS.iter().any(|k| lower.contains(k))
    {
        return QueryComplexity::Simple;
    }

    // Planning/architecture keywords → Planning
    if PLANNING_KEYWORDS.iter().any(|k| lower.contains(k)) {
        return QueryComplexity::Planning;
    }

    QueryComplexity::Standard
}

