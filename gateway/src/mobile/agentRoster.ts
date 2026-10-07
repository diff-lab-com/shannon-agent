/**
 * B0 — `shannon/agent.list` real roster: the host's configured agent
 * definitions, read from `~/.shannon/agents/*.toml` (the engine's
 * `AgentDefinition` files — see `crates/shannon-agents/src/agent_defs.rs` for
 * the authoritative format: `name` / `description` / `model` / …).
 *
 * Deliberately read-only and dumb:
 *  - only the user-global TOML dir is scanned (absolute `homedir()` path —
 *    the gateway's cwd is unreliable, so NO cwd-relative project dirs);
 *  - `~/.claude/agents/*.md` and project-level dirs are NOT scanned in v1 —
 *    the Markdown front-matter format needs its own parser and can join the
 *    roster in a later batch without a wire change (the shape already allows
 *    any subset of agents);
 *  - an unreadable directory or a file that fails to parse is skipped, never
 *    thrown — a broken definition must not blank or fail the whole roster;
 *  - every entry reports `status: "idle"` and `activity: []`: this surface
 *    describes CONFIGURED agents, not live processes (the engine has no
 *    per-agent process face the gateway could watch yet), so those keys are
 *    forward-compat placeholders with fixed values.
 *
 * `shannon/agent.detail` stays NOT_IMPLEMENTED, and `task.dispatch` keeps
 * rejecting every non-empty `agent_id`: there is no engine-side per-agent
 * routing face — accepting an agent_id would be a lie.
 */

import { readdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

import { parse } from "smol-toml";

/** One roster entry on the `shannon/agent.list` wire (v2.3). */
export interface RosterAgent {
  /** The definition's `name`, verbatim (the TOML value, not the filename). */
  id: string;
  /** Same string as `id` — the display name. */
  name: string;
  /** The definition's `description` — omitted when absent/empty. */
  role?: string;
  /** The definition's `model` — omitted when absent/empty. */
  model?: string;
  /** Fixed "idle": configured, not live (see module doc). */
  status: "idle";
  /** Always empty in v1 — forward-compat placeholder. */
  activity: never[];
}

/** Default scan target: `~/.shannon/agents` (absolute; never cwd-relative). */
export function defaultAgentDirs(): string[] {
  return [join(homedir(), ".shannon", "agents")];
}

/**
 * Load the agent roster from the given directories (defaults to
 * `~/.shannon/agents`). Never throws: unreadable dirs yield no entries and
 * unparseable files are skipped, so the phone always gets an honest list of
 * the definitions that actually parsed.
 */
export function loadAgentRoster(dirs?: string[]): RosterAgent[] {
  const agents: RosterAgent[] = [];
  for (const dir of dirs ?? defaultAgentDirs()) {
    let entries: string[];
    try {
      entries = readdirSync(dir, { withFileTypes: true })
        .filter((e) => e.isFile() && e.name.endsWith(".toml"))
        .map((e) => e.name)
        .sort();
    } catch {
      continue; // missing/unreadable dir → no entries from it
    }
    for (const file of entries) {
      try {
        const parsed = parse(readFileSync(join(dir, file), "utf8")) as {
          name?: unknown;
          description?: unknown;
          model?: unknown;
        };
        // `name` is required (the engine's serde would reject its absence
        // too) — without it there is no id, so the definition is useless.
        if (typeof parsed.name !== "string" || parsed.name.length === 0) continue;
        agents.push({
          id: parsed.name,
          name: parsed.name,
          ...(typeof parsed.description === "string" && parsed.description.length > 0
            ? { role: parsed.description }
            : {}),
          ...(typeof parsed.model === "string" && parsed.model.length > 0
            ? { model: parsed.model }
            : {}),
          status: "idle",
          activity: [],
        });
      } catch {
        // Unparseable/unreadable file → skip it, keep the rest of the roster.
      }
    }
  }
  return agents;
}
