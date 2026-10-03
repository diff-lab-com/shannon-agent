// Canned /diff payloads for the slash-commands journey (wave-2 J15).
//
// The real `get_session_git_diff` inspects the session's working directory;
// the demo backend has no repo to walk, so the four SlashResultCard diff
// shapes (notRepo / noChanges / truncated / full patch) need fixture data.
// The script schema has no diff field (and must stay minimal), so the seam
// lives HERE: while a ChatScript is armed, a FIRST seeded session whose id
// is the sentinel `diff:<case>` selects the matching fixture (see the
// scripted gate in handlers.ts `get_session_git_diff`).
//
// 铁律 4: the fixture table is only consulted under an armed seed — the
// un-scripted demo path keeps the historical not-repo default verbatim.
import type { GitDiffSummary } from '@/lib/tauri-api'

export const GIT_DIFF_FIXTURES: Record<string, GitDiffSummary> = {
  // Clean repo — the calm "nothing to see" card.
  nochanges: {
    is_repo: true,
    files: [],
    patch: '',
    truncated: false,
  },
  // A big repo where the backend capped the patch — the truncated flag.
  truncated: {
    is_repo: true,
    files: [
      { path: 'src/billing/invoice.rs', insertions: 180, deletions: 42 },
      { path: 'src/webhooks/stripe.rs', insertions: 96, deletions: 8 },
      { path: 'README.md', insertions: 12, deletions: 3 },
    ],
    patch: [
      'diff --git a/src/billing/invoice.rs b/src/billing/invoice.rs',
      'index 83db48f..bf269f4 100644',
      '@@ -1,4 +1,12 @@',
      '+ (patch capped by the backend — first 64 KiB only)',
    ].join('\n'),
    truncated: true,
  },
  // A reviewable patch — file rows plus the expandable patch block.
  patch: {
    is_repo: true,
    files: [
      { path: 'src/main.rs', insertions: 2, deletions: 1 },
      { path: 'README.md', insertions: 5, deletions: 0 },
    ],
    patch: [
      'diff --git a/src/main.rs b/src/main.rs',
      'index 83db48f..bf269f4 100644',
      '--- a/src/main.rs',
      '+++ b/src/main.rs',
      '@@ -12,7 +12,8 @@ fn main() {',
      '+    println!("hello wave 2");',
      '-    println!("hello");',
    ].join('\n'),
    truncated: false,
  },
}

/**
 * The scripted /diff lookup: when `seed` is armed and its FIRST session id
 * carries the `diff:<case>` sentinel, return the matching fixture (cloned);
 * unknown tags and unarmed/demo seeds answer null (caller keeps the default).
 */
export function scriptedGitDiffFixture(
  seed: { sessions?: Array<{ id?: string }> } | null | undefined,
): GitDiffSummary | null {
  const id = seed?.sessions?.[0]?.id ?? ''
  if (!id.startsWith('diff:')) return null
  const fixture = GIT_DIFF_FIXTURES[id.slice('diff:'.length)]
  return fixture ? JSON.parse(JSON.stringify(fixture)) as GitDiffSummary : null
}
