// Slash-menu skill entries — G1 P0-2.2.
//
// The composer's autocomplete used to be a hardcoded static table, so
// installed skills were invisible while typing `/`. This module merges the
// backend's `list_skills` (home + project directories) into the menu as
// slash-triggered entries, REPL-style. Any failure or timeout degrades to
// the static table alone — never an error surface.

import * as api from '@/lib/tauri-api'
import type { SkillInfo } from '@/types'
import { SLASH_COMMANDS, filterSlashCommands, type SlashCommand } from './commands'

/** One installed skill as the slash menu renders it. `trigger` includes the
 *  leading `/` (backend contract, REPL-consistent). */
export interface SlashSkillEntry {
  name: string
  description: string
  trigger: string
  source: string
}

export type SlashMenuItem =
  | { kind: 'command'; command: SlashCommand }
  | { kind: 'skill'; skill: SlashSkillEntry }

/** Map a backend SkillInfo into a menu entry (defensive on shape). */
export function toSlashSkillEntry(info: SkillInfo): SlashSkillEntry {
  return {
    name: info.name,
    description: info.description,
    trigger: info.trigger || `/${info.name}`,
    source: info.source,
  }
}

/**
 * Fetch installed skills for the menu. Resolves to [] on error OR timeout
 * (default 3s) so the static command table remains immediately usable —
 * the merge never blocks the composer on a slow backend.
 */
export async function fetchSlashSkills(timeoutMs = 3000): Promise<SlashSkillEntry[]> {
  const timeout = new Promise<null>(resolve => setTimeout(() => resolve(null), timeoutMs))
  try {
    const result = await Promise.race([api.listSkills(), timeout])
    if (!result) return []
    return result.map(toSlashSkillEntry)
  } catch {
    return []
  }
}

/** Names the static table already owns — skills never shadow them. */
function reservedCommandNames(): Set<string> {
  const names = new Set<string>()
  for (const cmd of SLASH_COMMANDS) {
    names.add(cmd.name)
    for (const alias of cmd.aliases ?? []) names.add(alias)
  }
  return names
}

/**
 * Merge the static command table with installed skills for the given query
 * (`/xy` shape). Commands come first (existing behavior); skills follow in
 * list order, deduplicated against static names/aliases and against each
 * other. Matching mirrors filterSlashCommands: prefix or substring on the
 * name, plus the full trigger (`/name`) against the raw query.
 */
export function mergeSlashMenu(skills: SlashSkillEntry[], query: string): SlashMenuItem[] {
  const items: SlashMenuItem[] = filterSlashCommands(query).map(command => ({
    kind: 'command' as const,
    command,
  }))
  if (skills.length === 0) return items

  const raw = query.replace(/^\//, '').toLowerCase()
  const reserved = reservedCommandNames()
  const seenSkills = new Set<string>()
  for (const skill of skills) {
    if (seenSkills.has(skill.name)) continue
    if (reserved.has(skill.name)) continue
    const name = skill.name.toLowerCase()
    const trigger = skill.trigger.toLowerCase()
    const matches =
      !raw ||
      name.startsWith(raw) ||
      name.includes(raw) ||
      trigger.startsWith(query.toLowerCase())
    if (!matches) continue
    seenSkills.add(skill.name)
    items.push({ kind: 'skill', skill })
  }
  return items
}
