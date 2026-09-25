// fileRefs — file-path detection for chat content and tool inputs
// (docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md §4 P0-B).
//
// Decision §5-4: P0 detects paths inside inline code and tool inputs only —
// the lowest-false-positive surface. Relative paths resolve against the
// active session working directory, and the chip only highlights when the
// backend existence probe confirms the file (hallucinated paths must not
// look clickable).

/** Tools whose `path`-like input mutates a file — the Diff-button set. */
export const FILE_MUTATING_TOOLS = new Set([
  'write_file',
  'edit_file',
  'apply_patch',
  'str_replace_editor',
  'replace',
])

/** Tool-input fields that carry a file path across the engine's tools. */
export const PATH_INPUT_FIELDS = ['path', 'file_path', 'filePath', 'filepath', 'notebook_path'] as const

const PATHY_EXT_RE =
  /\.(rs|ts|tsx|js|jsx|mjs|cjs|py|pyw|go|java|kt|kts|swift|rb|php|c|h|cpp|hpp|cc|hh|cs|fs|fsx|lua|vue|svelte|astro|sql|sh|bash|zsh|fish|ps1|psm1|bat|cmd|toml|yaml|yml|json|json5|xml|ini|cfg|conf|env|properties|gradle|proto|graphql|tf|hcl|md|markdown|mdx|html|htm|css|scss|sass|less|svg|mermaid|mmd|txt|csv|tsv|log|lock|mk|nix|el|clj|cljs|ex|exs|erl|hrl|hs|ml|mli|dart|r|jl|scala|groovy|pl|pm|dart|sol|move|circom)$/i

/**
 * Heuristic: does this token look like a file path? Conservative by
 * design — extension-bearing tokens qualify anywhere, extensionless ones
 * only when explicitly path-prefixed. URLs and `scheme:` tokens never do.
 */
export function looksLikeFilePath(token: string): boolean {
  const t = token.trim()
  if (!t || t.length > 512 || /\s/.test(t)) return false
  if (t.includes('://')) return false
  if (/^[a-z][a-z0-9+.-]*:/i.test(t)) return false
  if (PATHY_EXT_RE.test(t)) return true
  return /^(\/|\.\/|\.\.\/|~\/)/.test(t)
}

/**
 * Resolve a detected token to an absolute path for the existence probe.
 * Returns null when it cannot be resolved (`~` needs a home hint the UI
 * does not have; relative paths need a working directory). `..` segments
 * are resolved against the base's own segments, so leading `..` correctly
 * climbs out of the working dir.
 */
export function resolveFileRefPath(raw: string, workingDir: string | null): string | null {
  const t = raw.trim()
  if (!t) return null
  if (t.startsWith('/')) return t
  if (t.startsWith('~')) return null
  if (!workingDir) return null
  const segs = workingDir.replace(/\/+$/, '').split('/').filter(Boolean)
  for (const seg of t.split('/')) {
    if (seg === '' || seg === '.') continue
    if (seg === '..') {
      segs.pop()
      continue
    }
    segs.push(seg)
  }
  return '/' + segs.join('/')
}

/** Extract the path-ish input field from a tool call's input object. */
export function extractToolInputPath(toolInput: unknown): string | null {
  if (toolInput == null || typeof toolInput !== 'object') return null
  const obj = toolInput as Record<string, unknown>
  for (const field of PATH_INPUT_FIELDS) {
    const v = obj[field]
    if (typeof v === 'string' && v.trim()) return v.trim()
  }
  return null
}

/** Basename for chip titles / artifact tab labels. */
export function basenameOf(path: string): string {
  const idx = path.lastIndexOf('/')
  return idx >= 0 ? path.slice(idx + 1) : path
}

// -- active working directory (module-level ref, synced from Chat.tsx) ------

let activeWorkingDir: string | null = null

export function setActiveWorkingDir(dir: string | null): void {
  activeWorkingDir = dir
}

export function getActiveWorkingDir(): string | null {
  return activeWorkingDir
}
