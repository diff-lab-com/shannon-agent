# D6 — Self-Improvement Loop Design

**Status:** Planning / blocked on engine coordination
**Owner:** ed
**Last updated:** 2026-06-26
**Estimated effort:** 3–5 days UI + 3–5 days engine = 6–10 days total

## Why

Hermes Desktop's self-improvement loop is cited as its single biggest
differentiator (per `05c` competitive analysis): post-execution
evaluation writes procedural skills to disk, so the agent gets
measurably better at repetitive tasks over time (TokenMix benchmark
cites 40% efficiency gain).

Shannon already has the *substrate* for this — skills are a first-
class concept (`shannon-skills` crate, installed-skills UI under
Extensions → Skills) and `~/.shannon/skills/` exists. What's missing
is the *loop*: capture → synthesize → review → install.

The user's 2026-06-26 directive placed D6 in the "implement" bucket
alongside Plan Mode and Diff Preview. **This doc recommends deferring
full D6 implementation to a dedicated sprint.** Plan Mode and Diff
Preview were 1–1.5 days each and pure-UI; D6 is multi-day, crosses
the engine boundary, and needs design alignment with `shannon-code`.

## Scope (in / out)

**In scope (MVP):**
- Capture execution telemetry for each chat session (tool-call
  sequence, success/failure, duration, user edits before approval)
- After a session ends (or crosses a threshold), propose a candidate
  skill: "You've done `X` three times this week — save as a skill?"
- Approval gate UI: user reviews the proposed skill (name, trigger,
  procedure, example) and either approves (installs), edits, or
  rejects
- Approved skills land in `~/.shannon/skills/<uuid>/` with a manifest
- Skills library section distinguishes "Agent-authored" vs "Curated"
  skills with visual badge

**Out of scope (MVP):**
- Cross-user skill sharing / marketplace
- Skill versioning and conflict resolution
- Auto-pruning of low-performing skills
- Skill composition (skill A calls skill B)
- Learning model fine-tuning (purely procedural skills, not weights)

## UX design

### Trigger surfaces

The loop fires on three possible triggers:

1. **Session end:** User closes a chat session that had ≥3 tool calls.
   Background analysis runs; if a pattern is detected, show a toast:
   *"Shannon noticed a repeatable workflow. Review proposed skill?"*
2. **Explicit:** User opens Extensions → Skills → "Suggest skill from
   recent activity" button.
3. **Periodic:** Once weekly (configurable), if any patterns are
   detected, surface them in the OPC Tasks board.

### Approval gate modal

```
┌── Propose New Skill ────────────────────────────┐
│                                                 │
│  Shannon noticed you've done this 3× this week: │
│                                                 │
│  Name:    [Refactor React component        ]    │
│  Trigger: "when I ask to refactor a React       │
│           component..."                         │
│                                                 │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
│                                                 │
│  Procedure:                                     │
│  1. Read the target file                        │
│  2. Identify props + hooks                      │
│  3. Extract subcomponents →                     │
│  4. ... (full captured sequence)                │
│                                                 │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
│                                                 │
│  Example output:                                │
│  [diff snippet from one of the 3 captures]      │
│                                                 │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
│                                                 │
│  [Reject]  [Edit...]  [Save skill]              │
│                                                 │
└─────────────────────────────────────────────────┘
```

- **Edit** opens an inline editor for name/trigger/procedure
- **Save skill** writes to `~/.shannon/skills/<uuid>/skill.json`
- **Reject** logs a negative signal (adjusts future suggestion
  threshold for similar patterns)

### Skills library

The existing Extensions → Skills page gains a new filter pill:
"All / Curated / Agent-authored". Agent-authored skills show a
distinctive badge (icon: `auto_fix`) and an "Ancestry" link that
shows the originating sessions.

### Settings

- "Enable self-improvement" master toggle (default: off — privacy-
  conscious default)
- "Suggestion frequency": Real-time / Daily / Weekly / Manual
- "Skill storage path" (default `~/.shannon/skills/`)
- "Auto-approve skills matching past approvals" (default: off — every
  proposed skill requires explicit human approval)

## Technical design

### Telemetry capture

Each running session emits events already (`QueryEvent::ToolStart`,
`QueryEvent::ToolEnd`, etc. — see `src/events.rs`). The desktop
shell adds a new persistent subscriber that writes anonymized
summaries to `~/.shannon/skill-traces/<session-id>.jsonl`:

```jsonc
{
  "ts": "2026-06-26T13:14:00Z",
  "session_id": "abc123",
  "tool": "edit_file",
  "args_summary": { "path": "src/Foo.tsx", "lines_changed": 24 },
  "duration_ms": 1840,
  "outcome": "success",
  "user_edited_before_approval": false
}
```

The subscriber is a Tauri command `attach_skill_telemetry_listener`
registered once in `main.rs::setup()`.

### Pattern detection

A periodic background job (daily, runs on app startup if not yet run
today) scans recent traces and identifies repeated patterns:

- Same tool sequence (≥3 occurrences in 7 days)
- Similar args (e.g., same file-type + similar line counts)
- Successful outcomes

Pattern detection runs entirely in the desktop shell; no engine
involvement. Output: candidate skill manifests in
`~/.shannon/skill-candidates/<uuid>.json`.

### Skill synthesis (engine boundary)

Converting a candidate pattern into a readable procedure (steps 1–5
in the UX mockup) requires LLM synthesis. Two options:

**Option A (recommended):** Use the existing chat engine. The shell
sends a meta-prompt: "Here are 3 captured traces of similar work.
Synthesize a reusable procedure." Output is the skill body.

**Option B:** Add a dedicated `shannon-skills::synthesize` API in
the engine. Cleaner separation but requires engine changes.

Option A can ship first; Option B is a later refactor.

### Skill format

Matches the existing `shannon-skills` manifest schema:

```jsonc
{
  "id": "<uuid>",
  "name": "Refactor React component",
  "description": "...",
  "trigger": "user requests React component refactoring",
  "procedure": ["step 1", "step 2", "..."],
  "source": "agent-authored",
  "created_at": "<iso>",
  "originating_sessions": ["abc123", "def456", "..."]
}
```

### New Tauri commands

```rust
// src/commands_skills.rs (new module, or extend commands_mcp.rs)
#[tauri::command]
async fn list_skill_candidates() -> Result<Vec<SkillCandidate>>

#[tauri::command]
async fn approve_skill_candidate(id: String, edits: Option<SkillManifest>) -> Result<SkillManifest>

#[tauri::command]
async fn reject_skill_candidate(id: String) -> Result<()>

#[tauri::command]
async fn list_agent_authored_skills() -> Result<Vec<SkillManifest>>
```

### Frontend layer

```
ui/src/components/self-improve/
├── SkillApprovalModal.tsx     # the gate dialog
├── SkillBadge.tsx             # "Agent-authored" visual badge
├── SkillAncestryViewer.tsx    # shows originating sessions
├── SkillSuggestionToast.tsx   # surfacing trigger
└── useSkillCandidates.ts      # hook for polling

ui/src/pages/Skills.tsx         # existing page, adds filter pill
ui/src/pages/SkillReview.tsx    # new route for batch review
```

## Phased plan

### Phase 1 — Telemetry capture (2 days)

- Rust: `attach_skill_telemetry_listener` writes JSONL traces
- Rust: daily pattern-detection job writes skill candidates
- No UI yet — just observable artifacts on disk

Exit: After a week of normal use, `~/.shannon/skill-candidates/`
contains JSON files representing detected patterns.

### Phase 2 — Approval gate UI (1.5 days)

- Frontend: SkillApprovalModal with Edit / Reject / Save
- Backend: `list_skill_candidates`, `approve_skill_candidate`,
  `reject_skill_candidate`
- SkillSuggestionToast trigger on session-end detection

Exit: Full loop works end-to-end. User can review, edit, approve.

### Phase 3 — Skills library integration (1 day)

- Skills page filter pill (All / Curated / Agent-authored)
- SkillBadge component
- SkillAncestryViewer

Exit: Approved skills are first-class in the library.

### Phase 4 — Skill synthesis quality (1+ days)

- Replace naive procedure extraction with LLM synthesis via Option A
- Add example-output rendering (diff snippet)
- Iterate on prompt quality

Exit: Procedures read like human-written documentation.

### Phase 5 — Polish + privacy (1 day)

- Settings panel section
- "Pause self-improvement" quick toggle
- Data retention controls (delete traces older than N days)
- Clear data button

Exit: Privacy-conscious defaults; user feels in control.

## Risks

- **Privacy perception.** Users may find "the AI is watching me work"
  unsettling. Mitigation: default OFF, clear onboarding, retention
  controls, easy pause.
- **Noisy / low-quality suggestions.** Early pattern detection will
  produce garbage. Mitigation: high threshold (≥3 occurrences),
  LLM-synthesized procedures (Phase 4) dramatically improve quality.
- **Engine coordination.** Phase 4 (synthesis) crosses the engine
  boundary. Can be deferred — naive extraction works for MVP.
- **Skill conflicts.** Two skills trigger on similar patterns. Out
  of MVP scope; document as known limitation.
- **Disk space.** Traces accumulate. Default retention: 30 days,
  configurable.

## Acceptance criteria

- [ ] After 3 successful similar tool-call sequences in a week, the
      shell surfaces a skill suggestion
- [ ] Approval modal shows name, trigger, procedure, example
- [ ] Approved skills land in `~/.shannon/skills/` with correct
      manifest format
- [ ] Approved skills appear in Extensions → Skills with the
      "Agent-authored" badge
- [ ] Rejected suggestions don't reappear for the same pattern
- [ ] "Pause self-improvement" toggle stops all telemetry capture
- [ ] Clearing data removes all traces and candidates
- [ ] Settings allow choosing suggestion frequency

## Dependencies

- Engine: `shannon-skills` crate (already pinned) — uses existing
  skill manifest format, no changes required for Phase 1–3
- Engine: optional Phase 4 synthesis API (can defer; use chat engine)
- Tauri: no new plugins needed
- npm: no new frontend dependencies

## Open questions

- Should agent-authored skills be exportable / shareable? (Out of
  MVP scope but a natural follow-up.)
- How to handle skills that reference specific file paths? (Strip
  paths during synthesis? Generalize to patterns?)
- Should we A/B test different suggestion thresholds? (Defer to
  post-launch instrumentation.)

## Recommendation

**Do not implement D6 in the current sprint.** The scoping above
establishes that this is a 6–10 day cross-repo project that deserves
its own discovery pass. Plan Mode (D4) and Diff Preview (D5) were
the right scope for Week D's "implement" bucket.

A natural sequencing for a future sprint:
1. Week 1: Phase 1 (telemetry) + Phase 2 (approval UI) — minimum
   viable loop in the desktop shell only
2. Week 2: Phase 3 (library integration) + Phase 4 (LLM synthesis)
3. Week 3: Phase 5 (polish) + instrumentation for threshold tuning
