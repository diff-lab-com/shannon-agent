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
 *
 * Security: nothing here accepts inbound text from an unpaired device — the
 * `shannon/task.*` handlers gate on the bound session (PAIRING_REQUIRED), and
 * pushes only ever reach sockets that completed `shannon/pair` /
 * `shannon/device.resume`.
 */

import { WebSocket } from "ws";
import { sharedPushSeq, type SeqCounter } from "./seq.js";
import type { PushReplayBuffer } from "./pushReplay.js";

import {
  type ApprovalReq,
  type Logger,
  type NormalizedInbound,
} from "../adapters/types.js";
import { TASK_FAILED_STAMP_PREFIX, titleFromText } from "../router/lifecycle.js";
import {
  JSONRPC_VERSION,
  type MobileTaskRecord,
  type ShannonEvent,
  type UsageFrame,
} from "./protocol.js";
import type { MethodContext } from "./server.js";
import type { ApprovalRegistry } from "./approvalRegistry.js";

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
}

/**
 * §K wire projection of one journal record: `id` = task id (also the task
 * thread's session key), `prompt` = the dispatched text, `created_at` =
 * ISO-8601 UTC of `started_at`. `agent_id` is always null — this host has no
 * agent roster. The legacy P2-1 keys (device_id/title/text/started_at/…)
 * deliberately do NOT appear (spec §K2).
 */
export function wireTask(record: TaskRecord): MobileTaskRecord {
  return {
    id: record.task_id,
    prompt: record.text,
    status: record.status,
    agent_id: null,
    created_at: new Date(record.started_at).toISOString(),
  };
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
  }

  /** Current push seq head — feeds `shannon/snapshot` / `shannon/resume`. */
  get lastSeq(): number {
    return this.seq.current();
  }

  /** Late-bind the router entry point (see `InboundSubmit`). */
  setSubmit(fn: InboundSubmit): void {
    this.submit = fn;
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
  // ── approvals (phone decides via Y/N text) ─────────────────────────────────

  /**
   * Push an approval request to the device and wait for its Y/N reply (or the
   * timeout → deny). Called by the "mobile" adapter from inside the turn, so
   * the approval turn handler forwards the decision to the engine exactly as
   * it does for the IM adapters.
   */
  requestApproval(deviceId: string, req: ApprovalReq): Promise<"allow" | "deny"> {
    this.pushEvent(deviceId, {
      type: "approval.request",
      request_id: req.requestId,
      tool_name: req.toolName,
      tool_input: req.toolInput,
      description: req.description,
      is_destructive: req.isDestructive,
      diff_preview: req.diffPreview,
    });
    // §L2: the ask is now visible to the restore face until a settle resolves it.
    this.approvals?.record({
      requestId: req.requestId,
      toolName: req.toolName,
      toolInput: req.toolInput,
      description: req.description,
      isDestructive: req.isDestructive,
      diffPreview: req.diffPreview,
      ts: this.now(),
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
  }

  private finishTask(taskId: string, status: "completed" | "failed", error?: string): void {
    const task = this.journal.find((t) => t.task_id === taskId && t.status === "running");
    if (!task) return; // already terminal (e.g. flipped by a §K3 terminal push)
    task.status = status;
    task.finished_at = this.now();
    if (status === "failed") task.error = error ?? null;
    this.removeRunningTask(task.device_id, taskId);
  }

  // ── dispatch (phone → gateway → engine) ────────────────────────────────────

  /**
   * Handle one prompt from a paired device: journal a task, announce
   * `query.started` (session_id = the task id — the phone builds its thread
   * from the dispatch response's id, §K3), and run the turn in the device's
   * lane through the same inbound pipeline the IM adapters use. The returned
   * record resolves immediately; the structured task stream and the journal
   * transition land asynchronously.
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
   * §K: there is deliberately no Y/N-text approval branch here anymore — the
   * dispatch action ALWAYS creates a task; approvals are answered via the
   * signed `shannon/approval/decide` (see `settleApproval`).
   */
  dispatch(deviceId: string, text: string): DispatchOutcome {
    const record: TaskRecord = {
      task_id: this.newTaskId(),
      device_id: deviceId,
      title: titleFromText(text),
      text,
      status: "running",
      started_at: this.now(),
      finished_at: null,
      error: null,
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
