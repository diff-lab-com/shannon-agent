/**
 * Shared types for the Remotes settings section.
 *
 * Mirrors the camelCase DTOs in `desktop/src/commands_remote.rs` — the
 * desktop UI keeps hand-written types (see desktop/CLAUDE.md conventions).
 */

/** An SSH host candidate discovered from `~/.ssh/config` (read-only). */
export interface SshHostCandidate {
  alias: string
  user: string | null
  hostname: string | null
  port: number | null
}

/** A running Docker container from `docker ps`. */
export interface ContainerInfo {
  id: string
  names: string
  image: string
  status: string
}

/** A saved remote execution target (`~/.shannon/remotes.toml`). */
export interface RemoteTarget {
  name: string
  kind: 'ssh' | 'docker'
  host: string | null
  port: number | null
  user: string | null
  container: string | null
  shell: string | null
  sshTarget: string | null
  workspaceDir: string
}

/** Result of a connectivity probe (`remote_test_target`). */
export interface RemoteHealth {
  ok: boolean
  platform: string
  home: string
  bashAvailable: boolean
  workspaceExists: boolean
  latencyMs: number
  error: string | null
}
