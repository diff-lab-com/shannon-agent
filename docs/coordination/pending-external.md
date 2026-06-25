# Pending External Coordination

Items that cannot be executed inside this repo because they live in
another repo or depend on an external event. Tracked here so the next
session can pick them up without re-discovering the context.

## D1 — shannon-core split, Phase 2a (extract `shannon-engine`)

**Owner repo:** `shannon-agent/shannon-code` (GitHub)
**Design doc:** `docs/architecture/d1-shannon-core-split.md` (here)
**Status:** Phase 1 docs only (PR #4 in shannon-code). Phase 2a is
tracked as **issue #64** in shannon-code — extraction of the
`shannon-engine` crate per the design doc's three-stage migration path.

**Why it matters here:** when Phase 2a lands, Shannon Desktop's
`Cargo.toml` `[patch."ssh://..."]` block needs a new entry for
`shannon-engine`. The dependency already exists as a git dep at
`rev = "d49e7f5"` so the symbol migration is forward-compatible; the
patch entry is the only desktop-side change required.

**Action when Phase 2a lands:**
1. Bump pin in `Cargo.toml` (all `shannon-*` deps)
2. Add `shannon-engine = { path = "../shannon-code/crates/shannon-engine" }`
   to the `[patch]` block
3. Run `rm Cargo.lock && cargo generate-lockfile`
4. Verify with `cargo tree --package shannon-engine --depth=0` — should
   print the local path, not a `git+ssh://` URL
5. Smoke-test: `cargo build --release` and `pnpm --dir ui test --run`

Per memory `project_ci_release_gotchas.md` gotcha #2: missing patch
entries cause CI to fall through to `ssh://` and fail auth.

## D2 — PR #50 review feedback

**URL:** https://gitea.diff-lab.com/bigdong89/shannon-desktop/pulls/50
**Branch:** `s2/ui-design-overhaul` (pushed, 64 commits + 1 StreamingResponse
test commit `44cbb88`)
**State:** Open, awaiting first review comment.

**Watch for:**
- gitea email notifications (if configured)
- `tea pulls list --login diff-lab` shows PR #50 state
- Manual check: `tea pulls reviews 50 --login diff-lab`

**When feedback arrives:**
1. Read all comments before responding
2. Group by file to reduce review cycles
3. Push fixes as additional commits to `s2/ui-design-overhaul` (PR
   auto-updates)
4. Do not force-push unless reviewer requests a clean history

**If review blocks v0.3.7 tag** for >2 days, consider:
- Splitting PR #50 into smaller PRs (P0/P1 → PR-1, P2/P3 → PR-2)
- Or merge as-is and ship follow-up PR for unresolved audit items (see
  `docs/product-review/pm-audit-followups.md`)

## Related reactive items (not blockers)

- **cargo fmt drift on dev** — pre-existing, will be fixed at v0.3.7
  tag time per release prep doc.
- **release.yml engine pin drift** (`26343ba1` vs Cargo.toml's
  `d49e7f5`) — captured in `docs/release-notes/v0.3.7-prep.md` blocker #2.
