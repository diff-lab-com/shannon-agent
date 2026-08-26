// Pure helpers used across the Chat page subtree. No React imports so this
// file stays tree-shakeable and can be imported from server-side tests if
// needed.

// Collapse a long absolute path into a tail-only breadcrumb for the chip UI.
// "/home/alice/code/myproject" → "…/code/myproject"
export function formatDirBreadcrumb(full: string): string {
  const parts = full.replace(/\\/g, '/').split('/').filter(Boolean)
  if (parts.length <= 2) return full
  return '…/' + parts.slice(-2).join('/')
}
