// R2-P2-2 — structured hard-error tags on the send path.
//
// The invoke error channel is a frozen `String`, so tagged backend errors
// carry a machine kind on the SAME string: `shannon-error:<kind>|<original
// text>`. A known kind maps to localized copy; anything else — untagged
// engine errors, unknown kinds, old payloads — renders the original text
// verbatim. The tag is deliberately NOT rolled out beyond the send path's
// hard errors (brief: "不扩到全部后端错误").

const TAG_RE = /^shannon-error:([a-z0-9_]+)\|([\s\S]*)$/

/** Backend error kind → i18n key. Kinds the UI knows about; everything
 *  else falls back to the original text after the tag. */
const KIND_TO_KEY: Record<string, string> = {
  no_working_dir: 'chat.error.backend.noWorkingDir',
  query_in_progress: 'chat.error.backend.queryInProgress',
  goal_run_active: 'chat.error.backend.goalRunActive',
}

/** Split a tagged backend error. Null when the string carries no tag. */
export function parseBackendErrorTag(raw: string): { kind: string; original: string } | null {
  const match = TAG_RE.exec(raw)
  return match ? { kind: match[1], original: match[2] } : null
}

/**
 * Localize a backend error for the chat banner. A tagged, known kind
 * renders the mapped i18n message; an untagged string or an unknown kind
 * renders as-is (the original text travels with the tag for exactly this).
 * `messageFor` is injected (not imported) so callers can use either the
 * context-bound `useT` or the provider-independent `messageFor`.
 */
export function describeBackendError(
  raw: string,
  messageFor: (id: string) => string,
): string {
  const tag = parseBackendErrorTag(raw)
  if (!tag) return raw
  const key = KIND_TO_KEY[tag.kind]
  return key ? messageFor(key) : tag.original
}
