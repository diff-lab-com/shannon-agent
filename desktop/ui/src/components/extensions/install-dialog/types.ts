// InstallDialog shared types and helpers.
//
// T3.1 split: the dialog was a 510-line monolith with four bodies
// (OAuth featured_vendor, GitHub repo, MCP stdio registry, manual fallback).
// This module owns:
//   - The `AddonKind → route` map.
//   - `buildStdioSpec()` (npm/pip/docker → validated command + args).
//   - The `MaybeMeta` defensive view over `CatalogEntry.metadata`.

import type { AddonKind, CatalogEntry } from '@/types'
import { isValidPackageName } from '@/lib/packageValidation'

const KIND_ROUTE: Record<AddonKind, string> = {
  skill: '/extensions/skills',
  agent: '/extensions/agents',
  mcp: '/extensions/mcp-servers',
  plugin: '/extensions/plugins',
  data_source: '/extensions/data-sources',
}

export { KIND_ROUTE }

interface OAuthMeta {
  transport?: string
  endpoint?: string
  scopes?: string[]
  vendor?: string
}

interface PackageMeta {
  package?: { name?: string; type?: string }
}

export type MaybeMeta = OAuthMeta & PackageMeta

export function readMeta(entry: CatalogEntry): MaybeMeta {
  return (entry.metadata ?? {}) as MaybeMeta
}

/// Returns { command, args } for a registry package or null when the package
/// shape isn't one we recognise. Package name is strictly validated before
/// being placed in args so a malicious registry entry can't inject flags
/// (e.g. `--privileged`) or shell metacharacters.
export function buildStdioSpec(
  pkgType: string | undefined,
  pkgName: string | undefined,
): { command: string; args: string[] } | null {
  if (!pkgType || !pkgName) return null
  const kind = pkgType as 'npm' | 'pip' | 'docker'
  if (!isValidPackageName(kind, pkgName)) return null
  switch (kind) {
    case 'npm':
      return { command: 'npx', args: ['-y', pkgName] }
    case 'pip':
      return { command: 'pipx', args: ['run', pkgName] }
    case 'docker':
      return { command: 'docker', args: ['run', '-i', '--rm', pkgName] }
  }
}