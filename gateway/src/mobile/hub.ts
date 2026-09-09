/**
 * P2-1 mobile dispatch hub — the bridge that lifts connected paired phones
 * into the same inbound pipeline the IM adapters use (T9): dispatch text →
 * per-device lane → approval loop → lifecycle push back to the phone.
 *
 * Three jobs:
 *  - Connections: `MobileServer` and the relay host hand every new
 *    `MethodContext` to `registerConnection`; once pairing/resume binds a
 *    device session, the hub tracks deviceId → open sockets so gateway-side
 *    code can push events to the phone (the `ReplyTarget` for the "mobile"
 *    platform adapter is `{ platform: "mobile", chatId: deviceId }`).
 *  - Approvals: `requestApproval` pushes an `approval.request` event and parks
 *    a pending entry; the device answers with a plain Y/N text through
 *    `shannon/task.dispatch` (DingTalk `parseChoice` dialect, shared via
 *    `adapters/approvalChoice.ts`). Timeout resolves deny — same posture as
 *    the IM adapters under the engine's 300s approval timeout. The decision
 *    itself is forwarded to the engine by the approval turn handler, exactly
 *    like the IM channels; the connection is already device-authenticated
 *    (paired + POP), so no extra per-decision signature is required on this
 *    path (the direct `shannon/approval/decide` path keeps its Ed25519
 *    signature requirement).
 *  - Task journal: every dispatched task is recorded in an in-memory journal
 *    (running → completed/failed) so `shannon/task.list` can show the recent
 *    tasks with their gateway-side status. Failure is observed from the
 *    lifecycle ❌ stamp (single source: `TASK_FAILED_STAMP_PREFIX`) or from a
 *    rejected turn, whichever lands first.
 *
 * Security: nothing here accepts inbound text from an unpaired device — the
 * `shannon/task.*` handlers gate on the bound session (PAIRING_REQUIRED), and
 * pushes only ever reach sockets that completed `shannon/pair` /
 * `shannon/device.resume`.
 */

import { WebSocket } from "ws";

import {
  type ApprovalReq,
  type Logger,
  type NormalizedInbound,
} from "../adapters/types.js";
import { parseApprovalChoice } from "../adapters/approvalChoice.js";
import { TASK_FAILED_STAMP_PREFIX, titleFromText } from "../router/lifecycle.js";
import {
  JSONRPC_VERSION,
  type ShannonEvent,
} from "./protocol.js";
import type { MethodContext } from "./server.js";

/** How long a pushed approval waits for the device's Y/N before denying. */
const APPROVAL_TIMEOUT_MS = 300_000;

/** Journal cap — recent tasks only; oldest entries are dropped first. */
const JOURNAL_CAP = 100;

/** Gateway-side record for one dispatched task (wire shape: `MobileTaskRecord`). */
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

interface PendingApproval {
  requestId: string;
  settle: (choice: "allow" | "deny") => void;
}

/** Result of `MobileDispatchHub.dispatch`. */
export type DispatchOutcome =
  | { kind: "approval"; choice: "allow" | "deny" }
  | { kind: "task"; taskId: string };

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
}

export class MobileDispatchHub {
  private readonly logger: Logger;
  private readonly approvalTimeoutMs: number;
  private readonly now: () => number;
  private readonly newTaskId: () => string;

  /** deviceId → open, session-bound contexts. */
  private readonly byDevice = new Map<string, Set<MethodContext>>();
  /** Reverse index for unbind on socket close. */
  private readonly deviceOf = new Map<MethodContext, string>();
  /** deviceId → FIFO pending approvals (lane serialization keeps this at 1). */
  private readonly pending = new Map<string, PendingApproval[]>();
  private readonly journal: TaskRecord[] = [];
  private submit: InboundSubmit | null = null;

  constructor(opts: MobileDispatchHubOptions) {
    this.logger = opts.logger;
    this.approvalTimeoutMs = opts.approvalTimeoutMs ?? APPROVAL_TIMEOUT_MS;
    this.now = opts.now ?? Date.now;
    this.newTaskId = opts.newTaskId ?? (() => crypto.randomUUID());
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

  /** Push one ShannonEvent notification to every open socket of the device. */
  pushEvent(deviceId: string, event: ShannonEvent): boolean {
    const sockets = this.byDevice.get(deviceId);
    if (!sockets || sockets.size === 0) return false;
    const frame = JSON.stringify({
      jsonrpc: JSONRPC_VERSION,
      method: "shannon/event",
      params: event,
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
   * Push a plain text message to the device (the "mobile" adapter's send path).
   * A lifecycle failure stamp passing through here also flips the device's
   * newest running task to failed, keeping `shannon/task.list` honest.
   */
  sendText(deviceId: string, text: string): boolean {
    if (text.startsWith(TASK_FAILED_STAMP_PREFIX)) {
      this.markRunningFailed(deviceId, text.slice(TASK_FAILED_STAMP_PREFIX.length));
    }
    return this.pushEvent(deviceId, { type: "task.message", text });
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

  // ── task journal ───────────────────────────────────────────────────────────

  /** Recent tasks for one device, newest first. */
  listTasks(deviceId: string, limit = 20): TaskRecord[] {
    return this.journal.filter((t) => t.device_id === deviceId).slice(0, Math.max(1, limit));
  }

  /** Journal size (diagnostics / tests). */
  get journalSize(): number {
    return this.journal.length;
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
  }

  private finishTask(taskId: string, status: "completed" | "failed", error?: string): void {
    const task = this.journal.find((t) => t.task_id === taskId && t.status === "running");
    if (!task) return; // already terminal (e.g. flipped by the ❌ stamp)
    task.status = status;
    task.finished_at = this.now();
    if (status === "failed") task.error = error ?? null;
  }

  // ── dispatch (phone → gateway → engine) ────────────────────────────────────

  /**
   * Handle one text from a paired device. A Y/N reply while an approval is
   * pending resolves it (DingTalk pattern); anything else starts a task
   * through the same inbound pipeline the IM adapters use. The turn runs in
   * the device's lane (serialized per device) — the returned task id resolves
   * immediately; completion lands in the journal asynchronously.
   */
  dispatch(deviceId: string, text: string): DispatchOutcome {
    const pendingQueue = this.pending.get(deviceId);
    const choice = parseApprovalChoice(text);
    if (pendingQueue && pendingQueue.length > 0 && choice !== null) {
      const entry = pendingQueue.shift()!;
      if (pendingQueue.length === 0) this.pending.delete(deviceId);
      entry.settle(choice);
      this.logger.info(`mobile hub: approval ${entry.requestId} → ${choice} (device ${deviceId})`);
      return { kind: "approval", choice };
    }

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

    if (!this.submit) {
      // Wiring bug, not a runtime condition — fail loudly in the journal.
      this.finishTask(record.task_id, "failed", "gateway: dispatch pipeline not wired");
      return { kind: "task", taskId: record.task_id };
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
        this.finishTask(record.task_id, "failed", (err as Error)?.message ?? String(err)),
    );
    return { kind: "task", taskId: record.task_id };
  }
}
