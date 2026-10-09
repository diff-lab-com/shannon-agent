/**
 * P2-1 mobile dispatch hub — the bridge that lifts connected paired phones
 * into the same inbound pipeline the IM adapters use (T9): dispatch prompt →
 * per-device lane → approval loop → structured task stream pushed back to the
 * phone.
 *
 * Three jobs:
 *  - Connections: `MobileServer` and the relay host hand every new
 *    `MethodContext` to `registerConnection`; once pairing/resume binds a
 *    device session, the hub tracks deviceId → open sockets so gateway-side
 *    code can push events to the phone (the `ReplyTarget` for the "mobile"
 *    platform adapter is `{ platform: "mobile", chatId: deviceId }`).
 *  - Approvals: `requestApproval` pushes an `approval.request` event and parks
 *    a pending entry. Two settle paths: the device's signed
 *    `shannon/approval/decide` (wired through `settleApproval` by the
 *    bootstrap) and the 300s timeout → deny — same posture as the IM adapters
 *    under the engine's approval window. (The P2-1 Y/N-text-dialect settle was
 *    removed from the RPC face in §K: `shannon/task.dispatch` now always
 *    creates a task.) The decision itself is forwarded to the engine by the
 *    mobile turn handler, exactly like the IM channels.
 *  - Task journal + §K3 task stream: every dispatched task is recorded in an
 *    in-memory journal (running → completed/failed) so `shannon/task.list`
 *    shows recent tasks with their gateway-side status, and its streamed
 *    content is pushed to the initiating device as `shannon/event`s whose
 *    `session_id` IS the task's own id: `query.started` on acceptance,
 *    `task.progress` per engine text delta / usage frame, then the terminal
 *    `task.message` (final reply) or `query.failed` — never the IM lifecycle
 *    stamps (🚀/✅/❌), which stay an IM-channel-only concern.
 *  - P2-9 roster live status: a task accepted WITH an `agent_id` pushes a
 *    `shannon/agent.state` notification (`{agent: {…, status: "running",
 *    currentTask}}`) to that device at acceptance and `{…, status: "idle",
 *    currentTask: null}` at the task's first terminal — the mobile roster's
 *    agents stop being perpetually idle. Un-attributed tasks push nothing
 *    (no roster row owns them — "无归属归 host"); the push is the full
 *    roster map (the phone REPLACES the entry) and deliberately seq-free /
 *    outside the replay ring (see `AgentStateNotification` in protocol.ts).
 *
 * Security: nothing here accepts inbound text from an unpaired device — the
 * `shannon/task.*` handlers gate on the bound session (PAIRING_REQUIRED), and
 * pushes only ever reach sockets that completed `shannon/pair` /
 * `shannon/device.resume`.
 */

import { WebSocket } from "ws";
import { sharedPushSeq, type SeqCounter } from "./seq.js";
import type { PushReplayBuffer } from "./pushReplay.js";

/**
 * §O3 wake trigger — the desktop→relay Push-to-Wake leg plugs in here.
 * `seq` is the push cursor at fire time (the payload §O3 pins: {handle, seq}
 * is assembled relay-side from the device binding + this cursor).
 */
export type PushWakeSink = (deviceId: string, seq: number, event: ShannonEvent) => void;

/** §O3 trigger set: approval asks + turn terminals (the "phone in pocket" moments). */
const WAKE_EVENT_TYPES: ReadonlySet<ShannonEvent["type"]> = new Set([
  "approval.request",
  "query.completed",
  "query.failed",
  "query.cancelled",
]);

import {
  type ApprovalReq,
  type Logger,
  type NormalizedInbound,
} from "../adapters/types.js";
import { TASK_FAILED_STAMP_PREFIX, titleFromText } from "../router/lifecycle.js";
import { type RosterAgent } from "./agentRoster.js";
import {
  JSONRPC_VERSION,
  type MobileAgentState,
  type MobileTaskRecord,
  type ShannonEvent,
  type ToolFrame,
  type UsageFrame,
} from "./protocol.js";
import type { MethodContext } from "./server.js";
import { engineAgent, engineRisk, type ApprovalRegistry } from "./approvalRegistry.js";

/** How long a pushed approval waits for the device's Y/N before denying. */
const APPROVAL_TIMEOUT_MS = 300_000;

/** Journal cap — recent tasks only; oldest entries are dropped first. */
const JOURNAL_CAP = 100;

/**
 * Gateway-side record for one dispatched task. Internal shape (the wire shape
 * is the §K `MobileTaskRecord` — see `wireTask`).
 */
export interface TaskRecord {
  task_id: string;
  device_id: string;
  title: string;
  text: string;
  status: "running" | "completed" | "failed";
  started_at: number;
  finished_at: number | null;
  error: string | null;
  /**
   * B0: the roster agent this task was dispatched under — the `id` of a
   * `~/.shannon/agents/*.toml` definition, validated by `shannon/task.dispatch`
   * at accept time. Gateway-side ATTRIBUTION only (the engine has no per-agent
   * routing face and still executes the turn as the default engine); null when
   * the dispatch carried no usable agent_id.
   */
  agent_id: string | null;
}

/**
 * §K wire projection of one journal record: `id` = task id (also the task
 * thread's session key), `prompt` = the dispatched text, `created_at` =
 * ISO-8601 UTC of `started_at`. B0: `agent_id` is the roster agent the task
 * was dispatched under (validated against `shannon/agent.list` at accept
 * time), or null when the dispatch carried none — attribution, not engine
 * routing (the engine still executes as the default engine).
 *
 * §K2 revision (B1a, v2.3 additive): the legacy P2-1 keys (device_id/text/
 * started_at/…) stay off the wire, but three additive keys return — `title`
 * (the internal task title, whenever non-empty), `finished_at` (ISO-8601 UTC,
 * only once the record reached a terminal state) and `error` (only on
 * status=failed with a non-null error). All three are optional on the wire:
 * mobile mappers tolerate their absence, and a consumer of the old five-key
 * shape keeps working untouched.
 */
export function wireTask(record: TaskRecord): MobileTaskRecord {
  const wire: MobileTaskRecord = {
    id: record.task_id,
    prompt: record.text,
    status: record.status,
    agent_id: record.agent_id,
    created_at: new Date(record.started_at).toISOString(),
    // B1a: the internal title rides when non-empty (titleFromText guarantees
    // one today; an empty string stays omitted rather than shipped as "").
    ...(record.title.trim().length > 0 ? { title: record.title } : {}),
  };
  if (record.finished_at !== null) {
    wire.finished_at = new Date(record.finished_at).toISOString();
  }
  if (record.status === "failed" && record.error !== null) {
    wire.error = record.error;
  }
  return wire;
}

interface PendingApproval {
  requestId: string;
  settle: (choice: "allow" | "deny") => void;
}

/** Result of `MobileDispatchHub.dispatch` — the freshly created journal record. */
export type DispatchOutcome = {
  kind: "task";
  taskId: string;
  record: TaskRecord;
};

/** Where dispatched turns go — the router's `handleInbound`, injected late by
 *  the bootstrap (the router needs the registry, which needs the adapter,
 *  which needs this hub). */
export type InboundSubmit = (inbound: NormalizedInbound) => Promise<void>;

export interface MobileDispatchHubOptions {
  logger: Logger;
  /** Approval wait before denying (default 300s; tests shrink it). */
  approvalTimeoutMs?: number;
  /** Clock injection for tests. */
  now?: () => number;
  /** Test seam: task ids (default crypto.randomUUID). */
  newTaskId?: () => string;
  /** Test seam: the §K3 `query.started` turn id (default crypto.randomUUID). */
  newTurnId?: () => string;
  /**
   * WP-15 T4: push-notification seq counter (defaults to the process-wide
   * `sharedPushSeq`; tests inject their own for isolation).
   */
  seqCounter?: SeqCounter;
  /**
   * §L2: pending-approval registry. When set, `requestApproval` records every
   * pushed ask and every settle (timeout deny / Y/N text answer) resolves it —
   * keeping `shannon/approval.list` + the snapshot `pendingApprovals` honest.
   */
  approvals?: ApprovalRegistry;
  /**
   * §O4: per-device replay ring. When set, every seq-stamped push records
   * here (online or not) so `shannon/resume(sinceSeq)` can replay what the
   * phone missed while offline. Same instance the pairing handlers read.
   */
  replay?: PushReplayBuffer;
  /**
   * §O3: the Push-to-Wake trigger seam. Fired for `approval.request` and turn
   * terminals (`query.completed` / `query.failed` / `query.cancelled`) with
   * the target device, the push seq, and the event — the future desktop→relay
   * `wake(deviceId, seq)` leg plugs in here (relay-side does the 10s
   * debounce/merge per §O3, so the seam stays fire-and-forget). Deliberately
   * NOT wired into the direct-query stream loop: an interactive
   * `shannon/query` means the user is already looking at the phone.
   * Absent (today: no relay) → inert.
   */
  wake?: PushWakeSink;
  /**
   * P2-9: roster lookup for the `shannon/agent.state` push — resolves a
   * dispatched task's `agent_id` to its `shannon/agent.list` entry so the
   * push carries the FULL roster map (`id`/`name`/`role`/`model`): the phone
   * REPLACES the matching roster entry with the pushed map, so a minimal
   * `{id, status}` would visibly degrade `name`/`role`/`model` on the Fleet
   * screen. Called per push (not cached) so a roster edit takes effect
   * without a restart — the same read-per-use philosophy the dispatch
   * handler's agent_id validation follows. A `null`/absent resolution while
   * a resolver IS wired means the agent left the roster mid-task: the push
   * is SKIPPED (the phone ADDS unknown ids — pushing would fabricate a ghost
   * roster entry; the next `agent.list` converges the screen instead).
   * Without any resolver the push degrades to the minimal map
   * (`{id, name: id, status, activity: [], currentTask}`) — dev wirings
   * only; the bootstrap always wires the resolver.
   */
  rosterEntry?: (agentId: string) => RosterAgent | null;
}

export class MobileDispatchHub {
  private readonly logger: Logger;
  private readonly approvalTimeoutMs: number;
  private readonly now: () => number;
  private readonly newTaskId: () => string;
  private readonly newTurnId: () => string;
  private readonly seq: SeqCounter;
  private readonly approvals: ApprovalRegistry | null;
  private readonly replay: PushReplayBuffer | null;
  /** Mutable: the relay leg late-binds this (see `setWake`). */
  private wake: PushWakeSink | null;
  /** P2-9: roster resolver for the agent.state push (see the option doc). */
  private readonly rosterEntry: ((agentId: string) => RosterAgent | null) | null;

  /** deviceId → open, session-bound contexts. */
  private readonly byDevice = new Map<string, Set<MethodContext>>();
  /** Reverse index for unbind on socket close. */
  private readonly deviceOf = new Map<MethodContext, string>();
  /** deviceId → FIFO pending approvals (lane serialization keeps this at 1). */
  private readonly pending = new Map<string, PendingApproval[]>();
  private readonly journal: TaskRecord[] = [];
  /**
   * §K3: deviceId → FIFO of still-running task ids. The device's lane
   * serializes turns, so the oldest running task is the one whose engine
   * events are currently streaming — delta/terminal pushes attribute to it.
   */
  private readonly runningTasks = new Map<string, string[]>();
  private submit: InboundSubmit | null = null;

  constructor(opts: MobileDispatchHubOptions) {
    this.logger = opts.logger;
    this.approvalTimeoutMs = opts.approvalTimeoutMs ?? APPROVAL_TIMEOUT_MS;
    this.now = opts.now ?? Date.now;
    this.newTaskId = opts.newTaskId ?? (() => crypto.randomUUID());
    this.newTurnId = opts.newTurnId ?? (() => crypto.randomUUID());
    this.seq = opts.seqCounter ?? sharedPushSeq;
    this.approvals = opts.approvals ?? null;
    this.replay = opts.replay ?? null;
    this.wake = opts.wake ?? null;
    this.rosterEntry = opts.rosterEntry ?? null;
  }

  /** Current push seq head — feeds `shannon/snapshot` / `shannon/resume`. */
  get lastSeq(): number {
    return this.seq.current();
  }

  /** Late-bind the router entry point (see `InboundSubmit`). */
  setSubmit(fn: InboundSubmit): void {
    this.submit = fn;
  }

  /**
   * §O3: late-bind the wake sink — the relay leg assembles after the hub in
   * the bootstrap (the relay host connection exists only in relay mode), so
   * the constructor's `wake` option isn't always reachable from there.
   */
  setWake(fn: PushWakeSink): void {
    this.wake = fn;
  }

  // ── connections ────────────────────────────────────────────────────────────

  /**
   * Track a freshly accepted connection. Binding happens later, when
   * `shannon/pair` / `shannon/device.resume` sets the session; the returned
   * detach function removes it again (relay `peer_gone`, socket close).
   */
  registerConnection(ctx: MethodContext): () => void {
    ctx.onSessionBound = (deviceId: string) => {
      this.bind(ctx, deviceId);
    };
    const socket = ctx.socket;
    const onClose = (): void => this.unbind(ctx);
    socket.on("close", onClose);
    // A context may already carry a session (tests, or a hub attached after a
    // resume) — bind it up front.
    if (ctx.sessionId) this.bind(ctx, ctx.sessionId);
    return () => {
      socket.off("close", onClose);
      this.unbind(ctx);
    };
  }

  private bind(ctx: MethodContext, deviceId: string): void {
    this.unbind(ctx);
    let set = this.byDevice.get(deviceId);
    if (!set) {
      set = new Set();
      this.byDevice.set(deviceId, set);
    }
    set.add(ctx);
    this.deviceOf.set(ctx, deviceId);
    this.logger.info(`mobile hub: device ${deviceId} connected (${set.size} socket(s))`);
  }

  /** Devices with at least one bound connection (diagnostics / tests). */
  connectedDevices(): string[] {
    return [...this.byDevice.entries()].filter(([, s]) => s.size > 0).map(([d]) => d);
  }

  private unbind(ctx: MethodContext): void {
    const deviceId = this.deviceOf.get(ctx);
    if (!deviceId) return;
    this.deviceOf.delete(ctx);
    const set = this.byDevice.get(deviceId);
    set?.delete(ctx);
    if (set && set.size === 0) this.byDevice.delete(deviceId);
  }

  // ── push (gateway → phone) ─────────────────────────────────────────────────

  /** Number of open sockets the device will receive pushes on. */
  isDeviceConnected(deviceId: string): boolean {
    return (this.byDevice.get(deviceId)?.size ?? 0) > 0;
  }

  /**
   * Push one ShannonEvent notification to every open socket of the device.
   * WP-15 T4: every pushed notification carries a top-level `seq` (the
   * phone's live-sync cursor; a missing seq is invisible to it).
   * §O4: the push is recorded into the replay ring at seq-stamp time —
   * BEFORE the socket check, because an offline device's push is exactly
   * what `shannon/resume` replays later.
   */
  pushEvent(deviceId: string, event: ShannonEvent): boolean {
    const seq = this.seq.next();
    this.replay?.record(deviceId, seq, event);
    // §O3: wake on approval asks + turn terminals — fired BEFORE the socket
    // check on purpose: an offline device is exactly who needs waking.
    if (this.wake && WAKE_EVENT_TYPES.has(event.type)) {
      try {
        this.wake(deviceId, seq, event);
      } catch (err) {
        // A broken wake must never fail the push itself (same posture as the
        // §M2 device.revoked broadcast).
        this.logger.warn(`wake sink failed: ${(err as Error).message}`);
      }
    }
    const sockets = this.byDevice.get(deviceId);
    if (!sockets || sockets.size === 0) return false;
    const frame = JSON.stringify({
      jsonrpc: JSONRPC_VERSION,
      method: "shannon/event",
      params: { seq, ...event },
    });
    let delivered = false;
    for (const ctx of sockets) {
      if (ctx.socket.readyState !== WebSocket.OPEN) continue;
      ctx.socket.send(frame);
      delivered = true;
    }
    return delivered;
  }

  /**
   * Push a plain text message to the device — the "mobile" adapter's send
   * path. Legacy IM-bubble semantics (no session_id): since §K3 the task
   * stream does NOT ride this anymore (the mobile turn handler pushes
   * structured events instead), so on the live pipeline only an out-of-band
   * ❌ lifecycle stamp can still arrive here, and it keeps flipping the
   * newest running task to failed to stay honest.
   */
  sendText(deviceId: string, text: string): boolean {
    if (text.startsWith(TASK_FAILED_STAMP_PREFIX)) {
      this.markRunningFailed(deviceId, text.slice(TASK_FAILED_STAMP_PREFIX.length));
    }
    return this.pushEvent(deviceId, { type: "task.message", text });
  }

  /**
   * §M2: push one event to every connected device EXCEPT `exceptDeviceId` —
   * the fan-out for the `device.revoked` broadcast (the revoked device itself
   * must not hear it; it loses access at its next signed call anyway).
   * Devices without an open socket simply miss it (the phone's next
   * `device.list` converges the list — push is best-effort by design).
   */
  broadcastEvent(event: ShannonEvent, exceptDeviceId?: string): void {
    for (const [deviceId, sockets] of this.byDevice) {
      if (exceptDeviceId !== undefined && deviceId === exceptDeviceId) continue;
      if (sockets.size === 0) continue;
      this.pushEvent(deviceId, event);
    }
  }

  /**
   * P2-9: push one `shannon/agent.state` notification to every open socket
   * of the device — the roster live-status face the phone's Fleet screen
   * consumes (shannon-mobile `LiveAgentsNotifier._onAgentState`). A dedicated
   * notification METHOD, not a `shannon/event` type: the consumer filters
   * `n.method == Notifications.agentState` and reads `params.agent`. No seq,
   * no replay-ring record — see `AgentStateNotification` (protocol.ts).
   */
  private pushAgentState(deviceId: string, agent: MobileAgentState): boolean {
    const sockets = this.byDevice.get(deviceId);
    if (!sockets || sockets.size === 0) return false;
    const frame = JSON.stringify({
      jsonrpc: JSONRPC_VERSION,
      method: "shannon/agent.state",
      params: { agent },
    });
    let delivered = false;
    for (const ctx of sockets) {
      if (ctx.socket.readyState !== WebSocket.OPEN) continue;
      ctx.socket.send(frame);
      delivered = true;
    }
    return delivered;
  }

  /**
   * P2-9: the `shannon/agent.state` push for one agent-attributed task —
   * `running` with the task text at acceptance, `idle` with `currentTask:
   * null` at a terminal. Resolves the roster entry per push (see the
   * `rosterEntry` option for the miss/absent degradations); no-ops when the
   * task carries no `agent_id` — an un-attributed task belongs to the host,
   * and the phone's roster has no row whose status could honestly move.
   */
  private pushTaskAgentState(
    deviceId: string,
    task: TaskRecord,
    status: "running" | "idle",
  ): void {
    if (task.agent_id === null) return;
    const currentTask = status === "running" ? task.text : null;
    let agent: MobileAgentState;
    if (this.rosterEntry) {
      const entry = this.rosterEntry(task.agent_id);
      if (!entry) {
        // Resolver wired but the agent left the roster mid-task — skip
        // rather than ghost-add an unknown id on the phone (the phone's
        // consumer ADDS unknown ids; the next agent.list converges instead).
        return;
      }
      agent = {
        id: entry.id,
        name: entry.name,
        ...(entry.role !== undefined && entry.role.length > 0 ? { role: entry.role } : {}),
        ...(entry.model !== undefined && entry.model.length > 0 ? { model: entry.model } : {}),
        status,
        activity: [],
        currentTask,
      };
    } else {
      // No resolver wired (dev wirings): minimal honest map. `name: id` is
      // exactly what `agent.list` serves for the name.
      agent = { id: task.agent_id, name: task.agent_id, status, activity: [], currentTask };
    }
    this.pushAgentState(deviceId, agent);
  }

  /**
   * P2-9: the settle-side agent.state push, called once per agent-attributed
   * task at its FIRST terminal flip. The roster goes back to idle — unless
   * the device still has another running task under the SAME agent (the
   * device's lane queues them FIFO), in which case the oldest such queued
   * task is that agent's new `currentTask` and the roster stays `running`
   * instead of blinking idle between queue drains.
   */
  private settleTaskAgentState(task: TaskRecord): void {
    if (task.agent_id === null) return;
    const next = [...this.journal]
      .reverse() // journal is newest-first; the lane drains oldest-first
      .find(
        (t) =>
          t.device_id === task.device_id &&
          t.agent_id === task.agent_id &&
          t.status === "running" &&
          t.task_id !== task.task_id,
      );
    if (next) {
      this.pushTaskAgentState(task.device_id, next, "running");
    } else {
      this.pushTaskAgentState(task.device_id, task, "idle");
    }
  }

  // ── approvals (phone decides via Y/N text) ─────────────────────────────────

  /**
   * Push an approval request to the device and wait for its Y/N reply (or the
   * timeout → deny). Called by the "mobile" adapter from inside the turn, so
   * the approval turn handler forwards the decision to the engine exactly as
   * it does for the IM adapters.
   *
   * B1b: when the engine event carried the §L1 rich fields, the pushed event
   * AND the registry record carry them too — `agent`/`risk` are normalized
   * through the same `engineAgent`/`engineRisk` helpers the direct-query path
   * uses. The push carries `ts` only when the engine supplied it (the phone
   * stamps arrival time itself — no invention), while the record's `ts`
   * (required by the restore face) prefers `req.ts` and falls back to the hub
   * clock. Unusable values omit the key, so the push stays byte-identical to
   * the legacy shape on old engines.
   */
  requestApproval(deviceId: string, req: ApprovalReq): Promise<"allow" | "deny"> {
    const engineTs =
      typeof req.ts === "number" && Number.isFinite(req.ts) ? req.ts : null;
    const agent = engineAgent(req.agent);
    const risk = engineRisk(req.risk);
    this.pushEvent(deviceId, {
      type: "approval.request",
      request_id: req.requestId,
      tool_name: req.toolName,
      tool_input: req.toolInput,
      description: req.description,
      is_destructive: req.isDestructive,
      diff_preview: req.diffPreview,
      ...(engineTs !== null ? { ts: engineTs } : {}),
      ...(agent ? { agent } : {}),
      ...(risk ? { risk } : {}),
    });
    // §L2: the ask is now visible to the restore face until a settle resolves it.
    this.approvals?.record({
      requestId: req.requestId,
      toolName: req.toolName,
      toolInput: req.toolInput,
      description: req.description,
      isDestructive: req.isDestructive,
      diffPreview: req.diffPreview,
      ts: engineTs ?? this.now(),
      agent,
      risk,
    });
    return new Promise<"allow" | "deny">((resolve) => {
      let timer: NodeJS.Timeout | undefined;
      const entry: PendingApproval = {
        requestId: req.requestId,
        settle: (choice) => {
          if (timer) clearTimeout(timer);
          resolve(choice);
        },
      };
      timer = setTimeout(() => {
        this.removePending(deviceId, entry);
        // The engine itself times the request out to deny at 300s; denying here
        // keeps the reply loop unblocked when the phone never answers.
        this.approvals?.resolve(entry.requestId);
        entry.settle("deny");
      }, this.approvalTimeoutMs);
      this.pendingFor(deviceId).push(entry);
    });
  }

  /** Whether the device currently has a pending approval (diagnostics/tests). */
  hasPendingApproval(deviceId: string): boolean {
    return (this.pending.get(deviceId)?.length ?? 0) > 0;
  }

  private pendingFor(deviceId: string): PendingApproval[] {
    let q = this.pending.get(deviceId);
    if (!q) {
      q = [];
      this.pending.set(deviceId, q);
    }
    return q;
  }

  private removePending(deviceId: string, entry: PendingApproval): void {
    const q = this.pending.get(deviceId);
    if (!q) return;
    const i = q.indexOf(entry);
    if (i >= 0) q.splice(i, 1);
    if (q.length === 0) this.pending.delete(deviceId);
  }

  /**
   * Settle a parked `requestApproval` from a signed `shannon/approval/decide`
   * landing on the engine bridge (§K: the Y/N text dialect left the RPC face,
   * so this is the only path a waiting task lane unblocks besides the timeout).
   * The decide call is device-signed and engine-acknowledged before this runs,
   * so honoring the choice is exactly the user's decision. Idempotent: a
   * request that already settled (or never parked here) is a no-op.
   */
  settleApproval(requestId: string, choice: "allow" | "deny"): boolean {
    for (const [deviceId, queue] of this.pending) {
      const entry = queue.find((e) => e.requestId === requestId);
      if (!entry) continue;
      this.removePending(deviceId, entry);
      this.approvals?.resolve(requestId);
      entry.settle(choice);
      this.logger.info(`mobile hub: approval ${requestId} → ${choice} (device ${deviceId}, decide)`);
      return true;
    }
    return false;
  }

  /**
   * r2-w2d: deny-settle EVERY approval the device still has parked here — the
   * `shannon/cancel` entry point. A parked approval belongs to the cancelled
   * task's own turn, so once the device says "cancel my task" the ask can no
   * longer matter: leaving it parked would hold the lane inside the approval
   * round-trip until the device answers or the 300s timeout, delaying the
   * turn's `cancelled` terminal (§K3's `query.failed`) by up to 300s. Reuses
   * `settleApproval` verbatim (queue removal + registry resolve + waiter
   * release), so the race with the 300s timeout timer stays idempotent the
   * same way — single-threaded settle, first setter wins, and the loser's
   * `clearTimeout` / `Map.delete` / `resolve` are all no-ops. Approvals parked
   * for other devices are never touched. Returns the number actually settled
   * now (0 = nothing parked / already settled).
   */
  cancelPendingApprovals(deviceId: string): number {
    const queue = this.pending.get(deviceId);
    if (!queue || queue.length === 0) return 0;
    let settled = 0;
    for (const entry of [...queue]) {
      if (this.settleApproval(entry.requestId, "deny")) settled++;
    }
    return settled;
  }

  // ── task journal + §K3 task stream ─────────────────────────────────────────

  /** Recent tasks for one device, newest first. */
  listTasks(deviceId: string, limit = 20): TaskRecord[] {
    return this.journal.filter((t) => t.device_id === deviceId).slice(0, Math.max(1, limit));
  }

  /** Journal size (diagnostics / tests). */
  get journalSize(): number {
    return this.journal.length;
  }

  /** The oldest still-running task of a device (its stream is in flight). */
  private frontRunningTask(deviceId: string): string | null {
    const queue = this.runningTasks.get(deviceId);
    const head = queue?.[0];
    return head ?? null;
  }

  private takeRunningTask(deviceId: string): string | null {
    const queue = this.runningTasks.get(deviceId);
    if (!queue || queue.length === 0) return null;
    const taskId = queue.shift()!;
    if (queue.length === 0) this.runningTasks.delete(deviceId);
    return taskId;
  }

  private removeRunningTask(deviceId: string, taskId: string): void {
    const queue = this.runningTasks.get(deviceId);
    if (!queue) return;
    const i = queue.indexOf(taskId);
    if (i >= 0) queue.splice(i, 1);
    if (queue.length === 0) this.runningTasks.delete(deviceId);
  }

  /**
   * §K3: one engine text delta of the device's in-flight task stream →
   * `task.progress {session_id, content}` to the initiating device. No-op
   * when the device has no running task (stale event after a terminal).
   */
  pushTaskDelta(deviceId: string, content: string): void {
    const taskId = this.frontRunningTask(deviceId);
    if (!taskId) return;
    this.pushEvent(deviceId, { type: "task.progress", session_id: taskId, content });
  }

  /**
   * §K3 (revised 2026-10-09, tool-result cards §R): one engine tool frame of
   * the device's in-flight task stream → `task.progress {session_id, tool}`.
   * The original ruling ("the §K3 stream carries no tool frames") dates from
   * when nothing consumed them — the phone's ArtifactCard now does, and the
   * frame keys are all optional (old gateways never send, old phones ignore).
   * No-op when the device has no running task (stale event after a terminal).
   */
  pushTaskToolFrame(deviceId: string, tool: ToolFrame): void {
    const taskId = this.frontRunningTask(deviceId);
    if (!taskId) return;
    this.pushEvent(deviceId, { type: "task.progress", session_id: taskId, tool });
  }

  /**
   * §K3: the engine's usage frame → `task.progress {session_id, usage}`. The
   * phone accumulates session spend by conversation key; a task thread's key
   * is the task id.
   */
  pushTaskUsage(deviceId: string, usage: UsageFrame): void {
    const taskId = this.frontRunningTask(deviceId);
    if (!taskId) return;
    this.pushEvent(deviceId, { type: "task.progress", session_id: taskId, usage });
  }

  /**
   * §K3 terminal (success): flip the device's in-flight task to `completed`
   * BEFORE pushing (the mock pins `task.list` observing the transition as the
   * terminal lands), then push `task.message {session_id, text}` with the
   * final complete reply. Empty text suppresses the push (mirrors sendReply).
   */
  completeActiveTask(deviceId: string, finalText: string): void {
    const taskId = this.takeRunningTask(deviceId);
    if (!taskId) return;
    this.finishTask(taskId, "completed");
    if (finalText.length > 0) {
      this.pushEvent(deviceId, { type: "task.message", session_id: taskId, text: finalText });
    }
  }

  /**
   * §K3 terminal (failure observed by the turn handler from the engine's
   * `failed` event): flip the in-flight task to `failed` and push
   * `query.failed {session_id, error}` — no IM ❌ stamp on the task stream.
   */
  failActiveTask(deviceId: string, error: string): void {
    const taskId = this.takeRunningTask(deviceId);
    if (!taskId) return;
    this.finishTask(taskId, "failed", error);
    this.pushEvent(deviceId, { type: "query.failed", session_id: taskId, error });
  }

  private markRunningFailed(deviceId: string, stampBody: string): void {
    const task = this.journal.find(
      (t) => t.device_id === deviceId && t.status === "running",
    );
    if (!task) return;
    // The stamp is `formatTaskFailed(title, error)` = prefix + title + "\n" + error.
    const nl = stampBody.indexOf("\n");
    const error = (nl >= 0 ? stampBody.slice(nl + 1) : "").trim();
    task.status = "failed";
    task.finished_at = this.now();
    task.error = error || null;
    this.removeRunningTask(deviceId, task.task_id);
    // P2-9: the out-of-band ❌ stamp is a terminal too — the roster must not
    // stay running on a task the journal just failed.
    this.settleTaskAgentState(task);
  }

  private finishTask(taskId: string, status: "completed" | "failed", error?: string): void {
    const task = this.journal.find((t) => t.task_id === taskId && t.status === "running");
    if (!task) return; // already terminal (e.g. flipped by a §K3 terminal push)
    task.status = status;
    task.finished_at = this.now();
    if (status === "failed") task.error = error ?? null;
    this.removeRunningTask(task.device_id, taskId);
    // P2-9: first-terminal only (the early return above guards the dispatch
    // resolution's second finishTask pass) — idle, or the next queued task
    // of the same agent (see settleTaskAgentState).
    this.settleTaskAgentState(task);
  }

  // ── dispatch (phone → gateway → engine) ────────────────────────────────────

  /**
   * B0: the roster agent a task session belongs to — journal-derived
   * (`session_id` IS the task id on the §K3 stream and the engine session).
   * Feeds `shannon/session.list` attribution: task threads carry the agent
   * they were dispatched under. Null when the id is unknown (not dispatched
   * here / journal rolled over / gateway restarted) — the caller omits the
   * enrichment rather than inventing an owner.
   */
  agentForSession(sessionId: string): string | null {
    return this.journal.find((t) => t.task_id === sessionId)?.agent_id ?? null;
  }

  /**
   * Handle one prompt from a paired device: journal a task, announce
   * `query.started` (session_id = the task id — the phone builds its thread
   * from the dispatch response's id, §K3), and run the turn in the device's
   * lane through the same inbound pipeline the IM adapters use. The returned
   * record resolves immediately; the structured task stream and the journal
   * transition land asynchronously.
   *
   * B0 `agentId` (already roster-validated by the dispatch handler) is
   * recorded as ATTRIBUTION on the journal record — it rides the wire task
   * object and `agentForSession`; it is not engine routing (the turn still
   * executes as the default engine).
   *
   * §K1 ordering: the RPC response (the task object the phone keys its thread
   * by) must reach the wire BEFORE the event stream — the phone creates the
   * local bucket on the response and would clobber deltas that arrive first.
   * So the acceptance push and the turn submission are deferred to
   * `setImmediate`: the response frame is written in this macrotask's
   * microtask chain, the acceptance push happens at the check phase BEFORE
   * the turn starts, and every engine delta is written after it (the turn
   * handler's pushes run inside the submission's call tree).
   *
   * B0 engine-session fix: the turn's inbound carries `engineSessionId` =
   * the task's own UUID. The lane's default session key (`mobile:<deviceId>`)
   * is not a UUID and the engine's WS gate rejects every non-UUID session_id
   * query frame — pinning the task id keeps the dispatched turn's transcript
   * addressable (and `shannon/session.list` consistent with `agentForSession`).
   *
   * §K: there is deliberately no Y/N-text approval branch here anymore — the
   * dispatch action ALWAYS creates a task; approvals are answered via the
   * signed `shannon/approval/decide` (see `settleApproval`).
   */
  dispatch(deviceId: string, text: string, agentId: string | null = null): DispatchOutcome {
    const record: TaskRecord = {
      task_id: this.newTaskId(),
      device_id: deviceId,
      title: titleFromText(text),
      text,
      status: "running",
      started_at: this.now(),
      finished_at: null,
      error: null,
      agent_id: agentId,
    };
    this.journal.unshift(record);
    if (this.journal.length > JOURNAL_CAP) this.journal.length = JOURNAL_CAP;
    let queue = this.runningTasks.get(deviceId);
    if (!queue) {
      queue = [];
      this.runningTasks.set(deviceId, queue);
    }
    queue.push(record.task_id);

    setImmediate(() => {
      // 受理即推 (§K3 #1): the acceptance marker precedes any engine event.
      // The turn id is a fresh correlation id — the phone routes by session_id.
      this.pushEvent(deviceId, {
        type: "query.started",
        turn_id: this.newTurnId(),
        session_id: record.task_id,
      });
      // P2-9 受理即推: the roster agent this task is attributed to flips to
      // `running` on acceptance (never for un-attributed tasks — no roster
      // row owns them, the "无归属归 host" semantics stand).
      this.pushTaskAgentState(deviceId, record, "running");

      if (!this.submit) {
        // Wiring bug, not a runtime condition — fail loudly in the journal.
        this.abortTask(record, "gateway: dispatch pipeline not wired");
        return;
      }

      const inbound: NormalizedInbound = {
        platform: "mobile",
        chatId: deviceId,
        senderId: deviceId,
        senderName: "mobile",
        text,
        timestamp: this.now(),
        // The dispatch action is the trigger — no IM mention/prefix gating.
        isDirect: true,
        // B0: the turn's engine session IS the task (UUID) — see the doc above.
        engineSessionId: record.task_id,
      };
      this.submit(inbound).then(
        () => this.finishTask(record.task_id, "completed"),
        (err: unknown) =>
          this.abortTask(record, (err as Error)?.message ?? String(err)),
      );
    });
    return { kind: "task", taskId: record.task_id, record };
  }

  /**
   * A turn that died without reaching the handler's own §K3 terminal (engine
   * connect failure, handler throw): journal it failed AND close the phone's
   * stream with `query.failed {session_id}` so the thread doesn't hang open.
   */
  private abortTask(record: TaskRecord, error: string): void {
    this.finishTask(record.task_id, "failed", error);
    this.pushEvent(record.device_id, {
      type: "query.failed",
      session_id: record.task_id,
      error,
    });
  }
}
