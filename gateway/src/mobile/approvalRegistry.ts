/**
 * §L2 pending-approval registry (cross-repo adaptation spec) — the gateway's
 * in-memory record of every approval request currently awaiting a decision, so
 * a phone that reconnects (app restart, socket drop) can rebuild its approval
 * queue via `shannon/approval.list` or the `shannon/snapshot`
 * `pendingApprovals` array instead of losing the ask.
 *
 * Two producers feed it (the only places `approval.request` events originate):
 *  - the engine bridge's query stream (direct `shannon/query` turns), and
 *  - the dispatch hub's `requestApproval` (task turns via the IM pipeline).
 * Consumers resolve entries:
 *  - `shannon/approval/decide` success (the signed phone decision),
 *  - the hub's timeout settle (deny after 300s), and
 *  - the hub's Y/N text answer settle.
 *
 * Hygiene: entries are pruned lazily — older than `maxAgeMs` (the engine
 * itself denies approvals at 300s, so an older entry can no longer be
 * decided) or beyond `maxEntries` (oldest dropped). In-memory only: after a
 * gateway restart the engine still holds the ask, and the next
 * approval.request re-seeds the registry.
 */

import type { MobileApprovalItem } from "./protocol.js";

/** The engine's rich agent attribution (§L1) — absent on older engines. */
export interface EngineAgentInfo {
  id: string;
  name: string;
}

/** The engine's three-dimensional risk (§L1) — absent on older engines. */
export interface EngineRiskInfo {
  destructive?: boolean;
  scope: "local" | "repo" | "system";
  reversible: boolean;
}

/** One gateway-side pending approval. */
export interface PendingApprovalRecord {
  /** The engine's request id — the resolve key and the phone's `approvalId`. */
  requestId: string;
  toolName: string;
  /** Raw tool input, verbatim (never logged). */
  toolInput: unknown;
  description: string;
  isDestructive: boolean;
  diffPreview: string | null;
  /** Epoch ms when the request was recorded (the wire `timestamp` source). */
  ts: number;
  /** Engine attribution when present; absent keys are omitted on the wire. */
  agent?: EngineAgentInfo | null;
  risk?: EngineRiskInfo | null;
}

export interface ApprovalRegistryOptions {
  /** Clock injection for tests. */
  now?: () => number;
  /**
   * Entries older than this are swept from `listPending` (default 330s — the
   * engine denies at 300s, so anything staler can no longer be decided).
   */
  maxAgeMs?: number;
  /** Ring cap; the oldest entries are dropped first (default 200). */
  maxEntries?: number;
}

/** Default retention: engine deny-at-300s + a 30s settle margin. */
const DEFAULT_MAX_AGE_MS = 330_000;
const DEFAULT_MAX_ENTRIES = 200;

export class ApprovalRegistry {
  private readonly entries = new Map<string, PendingApprovalRecord>();
  private readonly now: () => number;
  private readonly maxAgeMs: number;
  private readonly maxEntries: number;

  constructor(opts: ApprovalRegistryOptions = {}) {
    this.now = opts.now ?? Date.now;
    this.maxAgeMs = opts.maxAgeMs ?? DEFAULT_MAX_AGE_MS;
    this.maxEntries = opts.maxEntries ?? DEFAULT_MAX_ENTRIES;
  }

  /** Record (or refresh) a pending approval. Re-recording an id is a no-op —
   *  the first push is the authoritative ask. */
  record(record: PendingApprovalRecord): void {
    if (this.entries.has(record.requestId)) return;
    this.sweep();
    this.entries.set(record.requestId, record);
    while (this.entries.size > this.maxEntries) {
      // Map preserves insertion order → the oldest key is first.
      const oldest = this.entries.keys().next().value;
      if (oldest === undefined) break;
      this.entries.delete(oldest);
    }
  }

  /** Drop a pending approval (decision landed). True when it was still pending. */
  resolve(requestId: string): boolean {
    return this.entries.delete(requestId);
  }

  /** Currently-pending approvals, oldest first, after the TTL sweep. */
  listPending(): PendingApprovalRecord[] {
    this.sweep();
    return [...this.entries.values()];
  }

  /** Number of live entries (diagnostics / tests). */
  get size(): number {
    return this.entries.size;
  }

  private sweep(): void {
    const cutoff = this.now() - this.maxAgeMs;
    for (const [id, rec] of this.entries) {
      if (rec.ts < cutoff) this.entries.delete(id);
    }
  }
}

/**
 * Normalize the engine event's `agent` field (`{id, name}`, either side
 * nullable, `#[serde(default)]`) into an `EngineAgentInfo`. Anything without a
 * usable id AND name → null (the wire key is then omitted, never invented).
 * Takes `unknown` so the record point never depends on the generated engine
 * types keeping pace.
 */
export function engineAgent(raw: unknown): EngineAgentInfo | null {
  if (typeof raw !== "object" || raw === null) return null;
  const { id, name } = raw as { id?: unknown; name?: unknown };
  if (typeof id !== "string" && typeof name !== "string") return null;
  return {
    id: typeof id === "string" ? id : "",
    name: typeof name === "string" ? name : "",
  };
}

/**
 * Normalize the engine event's `risk` field (`{scope, reversible[, destructive]}`)
 * into an `EngineRiskInfo`; null when the shape doesn't carry the required
 * scope/reversible pair.
 */
export function engineRisk(raw: unknown): EngineRiskInfo | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as { destructive?: unknown; scope?: unknown; reversible?: unknown };
  if (typeof r.scope !== "string" || typeof r.reversible !== "boolean") return null;
  if (r.scope !== "local" && r.scope !== "repo" && r.scope !== "system") return null;
  return {
    ...(typeof r.destructive === "boolean" ? { destructive: r.destructive } : {}),
    scope: r.scope,
    reversible: r.reversible,
  };
}

/**
 * §L2 wire mapping — one `MobileApprovalItem` for `shannon/approval.list` and
 * the snapshot `pendingApprovals` array. Keys are the phone's
 * `approvalFromMap` contract verbatim; optional engine-rich keys
 * (agentId/agentName/scope/diffTitle) are OMITTED, never invented, when the
 * source data is absent (the phone degrades honestly).
 */
export function approvalWireItem(rec: PendingApprovalRecord): MobileApprovalItem {
  const item: MobileApprovalItem = {
    approvalId: rec.requestId,
    kind: rec.toolName,
    headline: rec.description,
    risk: riskBand(rec),
    timestamp: new Date(rec.ts).toISOString(),
    toolInput: rec.toolInput,
  };
  if (rec.agent?.id) item.agentId = rec.agent.id;
  if (rec.agent?.name) item.agentName = rec.agent.name;
  if (rec.risk) item.scope = [rec.risk.scope];
  const path = toolInputPath(rec.toolInput);
  if (path !== undefined) item.diffTitle = path;
  return item;
}

/**
 * Risk band synthesis: the engine's three-dimensional risk (§L1) wins when
 * present — destructive or system-scoped is high, irreversible is medium,
 * otherwise low; without it the legacy `is_destructive` bit maps high/low.
 */
function riskBand(rec: PendingApprovalRecord): MobileApprovalItem["risk"] {
  const risk = rec.risk;
  if (risk) {
    if (risk.destructive === true || risk.scope === "system") return "high";
    if (!risk.reversible) return "medium";
    return "low";
  }
  return rec.isDestructive ? "high" : "low";
}

/** `tool_input.path` when the input is an object carrying a string path. */
function toolInputPath(toolInput: unknown): string | undefined {
  if (typeof toolInput !== "object" || toolInput === null) return undefined;
  const path = (toolInput as { path?: unknown }).path;
  return typeof path === "string" && path.length > 0 ? path : undefined;
}
