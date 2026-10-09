/**
 * §S group orchestration store (B6.0, cross-repo spec) — the host-side group
 * registry + per-group transcript, the data plane behind `shannon/group.*`.
 *
 * Storage ruling (2026-10-09, `docs/reviews/2026-10-09-mobile-proposals-rulings.md`
 * open question #2): groups are HOST-side orchestration entities, deliberately
 * NOT engine L0 sessions — a member turn's engine session is a one-shot UUID
 * and the group transcript only exists at the orchestration layer. Layout
 * (one directory per group; B6.1's `ledger.jsonl` will sit alongside):
 *
 *   ~/.shannon/groups/<groupId>/group.json      the entity (write-through)
 *   ~/.shannon/groups/<groupId>/transcript.jsonl  one JSON entry per line
 *
 * The phone replays a group thread through the §J2 face:
 * `shannon/session.history {sessionId: "grp-…"}` is intercepted by the
 * gateway (group key → transcript store; unknown key → engine fallback), so
 * the phone's routing chain is unchanged and `session.list` stays engine-only
 * (groups project through `shannon/group.list` — the two sources never mix).
 *
 * Everything here is synchronous small-file I/O: group counts are human-scale
 * and every mutation is a full `group.json` rewrite (temp + rename, the
 * engine trust-store discipline) plus a transcript line append.
 */

import { randomUUID } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  appendFileSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/** Group + member status vocabulary (the §S B6.0-2 response contract). */
export type GroupStatus = "active" | "completed" | "archived";
export type GroupMemberStatus =
  | "idle"
  | "queued"
  | "working"
  | "waiting-approval"
  | "done"
  | "failed"
  | "archived";

export interface GroupRules {
  /** Locked true: `false` is a create-time INVALID_PARAMS (the 14 red line). */
  paymentsAskFirst: true;
  /** R6: "need-only" — v1 enforces it prompt-level (orchestrator composes). */
  infoBoundary: string;
  /** Handoffs are confirmation-free; the red-line microcopy is fixed by the
   *  handoff event semantics (not a rule the user can turn off). */
  handoffFree: boolean;
  /** Local-timezone HH:mm; null = off. Stored in B6.0, enacted in B6.2. */
  dailyReportAt: string | null;
}

export interface GroupMember {
  memberId: string;
  slot: string;
  label: string;
  title: string;
  /** ephemeral = "临时 · 完成归档"; roster = attribution to a §A4 roster agent. */
  source: "ephemeral" | "roster";
  /** Roster path only; unknown ids are rejected at create (§K1 precedent). */
  agentId: string | null;
  shareCny: number;
  spentCny: number;
  status: GroupMemberStatus;
  statusNote: string | null;
  permissions: string[];
  /**
   * One-shot flag: after a handoff lands on this member, their NEXT approval
   * carries `group.ruleTrigger: "handoff-first"` (the R5 red line) and the
   * flag clears. Not part of the wire member object.
   */
  handoffFirstPending?: boolean;
}

export interface GroupPool {
  totalCny: number;
  spentCny: number;
  pendingCny: number;
  /** total − Σ shares at create time (the 14 "备用" row). */
  reserveCny: number;
}

export interface GroupRecord {
  groupId: string;
  title: string;
  goal: string;
  status: GroupStatus;
  createdAt: string;
  lastActivityAt: string;
  path: "ephemeral" | "roster";
  rules: GroupRules;
  pool: GroupPool;
  members: GroupMember[];
}

/** One transcript entry (the §J2 entry shape + group attribution keys). */
export interface GroupTranscriptEntry {
  role: "user" | "assistant";
  content: string;
  ts: string;
  /** user | member | system — absent degrades to the plain user style. */
  kind?: "user" | "member" | "system";
  /** Member attribution for kind:"member" entries (avatar + title row). */
  member?: { memberId: string; label: string; title: string };
}

/** Storage root override for tests; default `~/.shannon/groups`. */
export function defaultGroupsDir(): string {
  return join(homedir(), ".shannon", "groups");
}

export function newGroupId(): string {
  return `grp-${randomUUID()}`;
}

function groupDir(dirs: string[], groupId: string): string {
  return join(dirs[0] ?? defaultGroupsDir(), groupId);
}

function assertGroupId(groupId: string): void {
  // Path-safety first: ids are host-generated `grp-<uuid>`; anything else
  // ( traversal, empty ) never reaches the filesystem.
  if (!/^grp-[0-9a-fA-F-]{36}$/.test(groupId)) {
    throw new Error(`invalid groupId: ${groupId}`);
  }
}

export function loadGroup(dirs: string[], groupId: string): GroupRecord | null {
  assertGroupId(groupId);
  const path = join(groupDir(dirs, groupId), "group.json");
  if (!existsSync(path)) return null;
  try {
    return JSON.parse(readFileSync(path, "utf8")) as GroupRecord;
  } catch {
    // A corrupt entity file is an honest miss (the phone treats the group as
    // absent), never a crash.
    return null;
  }
}

export function saveGroup(dirs: string[], record: GroupRecord): void {
  assertGroupId(record.groupId);
  const dir = groupDir(dirs, record.groupId);
  mkdirSync(dir, { recursive: true });
  // Temp + rename so a crash never leaves a half-written entity.
  const tmp = join(dir, `group.json.tmp-${process.pid}`);
  writeFileSync(tmp, JSON.stringify(record, null, 2));
  renameSync(tmp, join(dir, "group.json"));
}

export function listGroups(dirs: string[]): GroupRecord[] {
  const root = dirs[0] ?? defaultGroupsDir();
  if (!existsSync(root)) return [];
  const out: GroupRecord[] = [];
  for (const name of readdirSync(root, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)) {
    const record = loadGroup(dirs, name);
    if (record) out.push(record);
  }
  return out;
}

export function appendTranscript(dirs: string[], groupId: string, entry: GroupTranscriptEntry): void {
  assertGroupId(groupId);
  const dir = groupDir(dirs, groupId);
  mkdirSync(dir, { recursive: true });
  appendFileSync(join(dir, "transcript.jsonl"), `${JSON.stringify(entry)}\n`);
}

export function readTranscript(dirs: string[], groupId: string): GroupTranscriptEntry[] {
  assertGroupId(groupId);
  const path = join(groupDir(dirs, groupId), "transcript.jsonl");
  if (!existsSync(path)) return [];
  const out: GroupTranscriptEntry[] = [];
  for (const line of readFileSync(path, "utf8").split("\n")) {
    const trimmed = line.trim();
    if (trimmed.length === 0) continue;
    try {
      out.push(JSON.parse(trimmed) as GroupTranscriptEntry);
    } catch {
      // Skip a torn tail line (crash mid-append) — the transcript is
      // append-only, so everything before it is intact.
    }
  }
  return out;
}

/**
 * §J2-flavored paging over the group transcript: `before` (ISO ts) anchors at
 * its FIRST occurrence and the page is the `limit` entries strictly older,
 * ascending; `hasMore` reports whether still-older entries exist. An unknown
 * anchor answers the latest page (the engine's unknown-cursor posture).
 */
export function pageTranscript(
  entries: GroupTranscriptEntry[],
  opts: { before?: string; limit?: number },
): { entries: GroupTranscriptEntry[]; hasMore: boolean } {
  const limit = Math.max(1, Math.floor(opts.limit ?? 50));
  let end = entries.length;
  if (typeof opts.before === "string" && opts.before.length > 0) {
    const idx = entries.findIndex((e) => e.ts === opts.before);
    if (idx >= 0) end = idx;
  }
  const start = Math.max(0, end - limit);
  return {
    entries: entries.slice(start, end),
    hasMore: start > 0,
  };
}
