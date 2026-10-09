/**
 * §S group orchestration (B6.0 起步 + B6.2 收尾, cross-repo spec) — the
 * `shannon/group.*` method face + the deterministic member-turn orchestrator.
 *
 * Wire posture (B-batch discipline): everything here is ADDITIVE — an old
 * gateway answers METHOD_NOT_FOUND for all five methods (the phone's honest
 * "预研能力" degrade), and every EVENT/key introduced here is optional on the
 * `shannon/event` channel. Rulings live in
 * `docs/reviews/2026-10-09-mobile-proposals-rulings.md`; the contract lands
 * in the mobile repo's spec as §S when the mono PR merges.
 *
 * What v1 honestly is:
 *  - Members are ATTRIBUTION + accounting labels, not engine personas (the
 *    B0 归属非路由 precedent): a member turn executes as the default engine
 *    under a one-shot UUID session (the engine WS gate rejects non-UUID
 *    session ids), re-keyed onto the group thread by the orchestrator.
 *  - Handoffs are DETERMINISTIC orchestrator steps (plan order + completion
 *    events), not member-initiated agency; the red line (R5) rides along as
 *    a one-shot `handoff-first` flag feeding the next approval's
 *    `group.ruleTrigger`.
 *  - Payments-ask-first is stored and locked at create, but v1 has no
 *    payment-class tool producer, so no `payments-ask-first` trigger is ever
 *    emitted (宁缺勿造). B6.1 pool accounting rides the SAME approval face:
 *    asks whose tool input carries a parseable amount enter the pool's
 *    pending column, escalate as `over-pool`/`over-share` (one trigger key
 *    per ask, handoff-first first), and on an ALLOW settle into the group
 *    ledger (`ledger.jsonl` + a `group.pool-spend` broadcast). Deny/expiry
 *    refund the pending and leave the ledger untouched.
 *  - The B6.0-4 TTL mechanism is fully live (engine `approval_ttl_ms`, hub
 *    per-ask deadline, registry entry-level retention, quote-expired card)
 *    but has NO quoteWindow producer yet: group asks keep the exact legacy
 *    300s window on every side until a payments connector exists.
 *  - B6.2 收尾: `group.get` serves the entity + the persisted review (16 屏);
 *    `group.archive {reason:"completed"}` aggregates + persists that review;
 *    the R11 daily report rides the per-minute tick in `groupReview.ts`; and
 *    an ephemeral create without members runs ONE engine planning query,
 *    degrading to the generic 3-slot template on any failure (诚实降级).
 *
 * Events fan out to EVERY connected device (`hub.broadcastEvent`): a group is
 * a host-level entity, not a device-private task thread — deliberately
 * different from §K3's initiating-device-only task streams.
 */

import { randomUUID } from "node:crypto";

import { respondToApproval } from "../engine/httpClient.js";
import { type EngineEvent } from "../engine/runtime.js";
import { EngineWsClient } from "../engine/wsClient.js";
import { type Logger } from "../adapters/types.js";
import { loadAgentRoster } from "./agentRoster.js";
import {
  appendPoolSpend,
  appendTranscript,
  listGroups,
  loadGroup,
  newGroupId,
  newPoolSpendId,
  pageTranscript,
  readTranscript,
  resolvePoolSpendAmountCny,
  round2Cny,
  saveGroup,
  type GroupMember,
  type GroupRecord,
  type GroupRules,
  type GroupTranscriptEntry,
  type PoolSpendEntry,
} from "./groupStore.js";
import {
  aggregateGroupReview,
  createDailyReportTimer,
  type DailyReportTimer,
} from "./groupReview.js";
import { ShannonError,
  type GroupArchiveParams,
  type GroupArchiveResult,
  type GroupCreateParams,
  type GroupCreateResult,
  type GroupGetParams,
  type GroupGetResult,
  type GroupListItem,
  type GroupListResult,
  type GroupMessageParams,
  type GroupMessageResult,
  type GroupObject,
  type GroupMemberInfo,
  type GroupReview,
} from "./protocol.js";
import type { MethodContext, MethodHandlers } from "./server.js";
import type { MobileTranscriptWire } from "./engineSessions.js";
import type { MobileDispatchHub } from "./hub.js";

const PAIRING_REQUIRED = {
  kind: "error" as const,
  code: ShannonError.PAIRING_REQUIRED,
  message: "pair a device first (shannon/pair or shannon/device.resume)",
};

const GROUP_ID_RE = /^grp-[0-9a-fA-F-]{36}$/;

/** Budget for the one-shot crew-planning query (open question #3 收尾). */
const DEFAULT_PLANNING_TIMEOUT_MS = 30_000;

export interface GroupHandlersOptions {
  hub: MobileDispatchHub;
  logger: Logger;
  /**
   * Group storage roots (default `~/.shannon/groups`). Tests inject a tmp
   * dir; array-shaped to mirror the agentRosterDirs seam.
   */
  groupsDirs?: string[];
  /** Roster validation targets for the create path (same seam as task.dispatch). */
  agentRosterDirs?: string[];
  /** Member-turn engine connection (the §K pipeline's construction). */
  engineWsUrl: string;
  /** Approval POST target (§P1-13 token posture). */
  engineHttpBaseUrl: string;
  engineAuthToken?: string | null;
  defaultModel?: string | null;
  /** Test seams (mirror dispatchPipeline). */
  fetchImpl?: typeof fetch;
  engineClientFactory?: (sessionKey: string) => EngineWsClient;
  now?: () => number;
  /** B6.2: budget for the create-time crew-planning query (default 30s). */
  planningTimeoutMs?: number;
  /** B6.2 test seams for the daily-report tick (see groupReview.ts). */
  reportScheduleNext?: (fn: () => void, delayMs: number) => () => void;
  reportTickIntervalMs?: number;
}

/** What `createGroupHandlers` hands back: the method face + the shutdown hook. */
export interface GroupHandlersBundle {
  /** The `shannon/group.*` methods — merged into the mobile handler map. */
  handlers: MethodHandlers;
  /**
   * Host-shutdown hook: clears the B6.2 daily-report tick chain. NOT a wire
   * method — bootstrap/dev-standalone call it from their stop path.
   */
  stop(): void;
}

/** CNY money on the wire is 元 with two decimals (§S 总则 7). */
function cny(v: unknown): number | null {
  if (typeof v !== "number" || !Number.isFinite(v) || v < 0) return null;
  return Math.round(v * 100) / 100;
}

function isoNow(opts: { now?: () => number }): string {
  return new Date(opts.now?.() ?? Date.now()).toISOString();
}

/** The deterministic v1 "planning query" stand-in: the generic 3-slot crew. */
function templateMembers(): Array<Pick<GroupMember, "slot" | "label" | "title"> & { permissions: string[] }> {
  return [
    { slot: "plan", label: "A", title: "规划分工", permissions: ["read.only"] },
    { slot: "do", label: "M", title: "执行", permissions: ["read.only"] },
    { slot: "verify", label: "S", title: "核对交付", permissions: ["read.only"] },
  ];
}

function memberWire(m: GroupMember): GroupMemberInfo {
  return {
    memberId: m.memberId,
    slot: m.slot,
    label: m.label,
    title: m.title,
    source: m.source,
    agentId: m.agentId,
    shareCny: m.shareCny,
    spentCny: m.spentCny,
    status: m.status,
    statusNote: m.statusNote,
    permissions: m.permissions,
  };
}

function groupWire(record: GroupRecord): GroupObject {
  return {
    groupId: record.groupId,
    title: record.title,
    goal: record.goal,
    status: record.status,
    createdAt: record.createdAt,
    members: record.members.map(memberWire),
    pool: { ...record.pool },
    rules: { ...record.rules },
    // B6.2: rides ONLY where the contract carries it (group.get, and the
    // archive response's own key) — a fresh create/list never has one.
    ...(record.review ? { review: record.review } : {}),
  };
}

export function createGroupHandlers(opts: GroupHandlersOptions): GroupHandlersBundle {
  const hub = opts.hub;
  const dirs = opts.groupsDirs ?? [];
  const now = () => opts.now?.() ?? Date.now();
  /** Per-group turn-chain mutex — a group runs one deterministic chain at a
   *  time; a second message while a chain runs queues behind it. */
  const chains = new Map<string, Promise<void>>();
  /** B6.2: the daily-report tick (R11). Armed lazily — it runs only while an
   *  active group has `rules.dailyReportAt` configured — and stopped via the
   *  bundle's stop() from the host shutdown chain. */
  const reports: DailyReportTimer = createDailyReportTimer({
    hub,
    logger: opts.logger,
    groupsDirs: opts.groupsDirs,
    now,
    scheduleNext: opts.reportScheduleNext,
    tickIntervalMs: opts.reportTickIntervalMs,
  });
  reports.arm();

  function broadcast(event: Parameters<MobileDispatchHub["broadcastEvent"]>[0]): void {
    hub.broadcastEvent(event);
  }

  function persist(record: GroupRecord): void {
    record.lastActivityAt = new Date(now()).toISOString();
    saveGroup(dirs, record);
  }

  /** B6.2: the review aggregate for a completing group — null (absent) when
   *  the transcript can't be read, never a fabricated one. */
  function buildReview(record: GroupRecord): GroupReview | null {
    try {
      return aggregateGroupReview(record, readTranscript(dirs, record.groupId));
    } catch (err) {
      opts.logger.warn(
        `group.archive: review aggregation failed (review omitted): ${(err as Error).message}`,
      );
      return null;
    }
  }

  function transcriptAdd(record: GroupRecord, entry: Omit<GroupTranscriptEntry, "ts">): void {
    appendTranscript(dirs, record.groupId, { ...entry, ts: new Date(now()).toISOString() });
    record.lastActivityAt = new Date(now()).toISOString();
  }

  function memberStatusEvent(record: GroupRecord, member: GroupMember): void {
    broadcast({
      type: "group.member",
      session_id: record.groupId,
      member: {
        memberId: member.memberId,
        status: member.status,
        statusNote: member.statusNote,
      },
    });
  }

  function systemEvent(record: GroupRecord, kind: string, text: string): void {
    broadcast({
      type: "group.system",
      session_id: record.groupId,
      system: { kind, text, ts: new Date(now()).toISOString() },
    });
    // `systemKind` is the host-local aggregation marker (B6.2): the review /
    // daily-report aggregators count REAL card kinds (e.g. handoff) instead
    // of pattern-matching prose. Never mapped onto the §J2 wire.
    transcriptAdd(record, { role: "assistant", content: text, kind: "system", systemKind: kind });
    persist(record);
  }

  // ── §S B6.0-4: the group-expired card ───────────────────────────────────
  // When a GROUP approval's window expires at the hub, the hub denies and
  // hands the ask back here (minimal intrusion: the hub owns no group
  // context, the orchestrator does — same late-binding posture as setWake).
  // The card is broadcast + transcribed like every system card. Plain
  // (groupless) approvals never invoke the sink: their expiry stays
  // invisible, the legacy behavior — no new cards on ordinary asks.
  hub.setGroupApprovalExpired((req) => {
    const group = req.group;
    if (!group) return;
    const record = loadGroup(dirs, group.groupId);
    if (!record) return; // group gone — nothing honest left to announce on
    const member = record.members.find((m) => m.memberId === group.member.memberId);
    const who = member ? `${member.title}（${member.label}）` : group.member.title;
    systemEvent(
      record,
      "quote-expired",
      `成员「${who}」的审批「${req.description}」在窗口内未决定，已自动放弃。`,
    );
  });

  // ── §S B6.1: pool ledger helpers (the honest-money path) ────────────────

  /**
   * The B6.1 escalation verdict for one ask, computed BEFORE the ask goes
   * out and only when its amount is resolvable (an unresolvable ask is
   * pool-invisible and can escalate nothing):
   *  - `over-pool` — spent + pending + this ask would pass the pool total;
   *  - `over-share` — this ask passes the member's remaining share
   *    (shareCny − spentCny).
   * Both hitting → `over-pool` carries the key (the host-level limit is the
   * louder alarm); the single-trigger-key rule stands either way. Neither
   * verdict BLOCKS the ask — approvals are the user's decision; the trigger
   * is the honest label on it (v1 enforcement is visibility, per §S).
   */
  function overRuleTrigger(
    record: GroupRecord,
    member: GroupMember,
    amountCny: number | null,
  ): "over-pool" | "over-share" | undefined {
    if (amountCny === null) return undefined;
    const cents = (v: number): number => Math.round(v * 100);
    const amount = cents(amountCny);
    const overPool =
      cents(record.pool.spentCny) + cents(record.pool.pendingCny) + amount >
      cents(record.pool.totalCny);
    const overShare = cents(member.shareCny) - cents(member.spentCny) < amount;
    if (overPool) return "over-pool";
    if (overShare) return "over-share";
    return undefined;
  }

  /**
   * B6.1 settlement for one amount-bearing ask, after the hub resolves it.
   * deny/expiry → the pending column refunds and the ledger takes nothing
   * (拒绝不记账); allow → ledger line first (the money book is the truth),
   * then the pool/member numbers move together and persist — the group.list
   * projection's `spentCny` always equals the ledger's running sum.
   * `decidedBy` records the settling device; the initiator fallback exists
   * only because the type allows a null decide (an allow always arrives via
   * a signed decide in practice — timeouts settle as deny and never reach
   * this branch).
   */
  function settlePoolSpend(
    record: GroupRecord,
    member: GroupMember,
    ev: { request_id: string; tool_name: string },
    settled: { choice: "allow" | "deny"; decidedBy: string | null },
    amountCny: number,
    initiatorDeviceId: string,
  ): void {
    if (settled.choice !== "allow") {
      record.pool.pendingCny = round2Cny(record.pool.pendingCny - amountCny);
      persist(record);
      return;
    }
    const poolAfter = round2Cny(record.pool.spentCny + amountCny);
    const entry: PoolSpendEntry = {
      id: newPoolSpendId(),
      memberId: member.memberId,
      amountCny,
      kind: ev.tool_name,
      approvalId: ev.request_id,
      decidedBy: settled.decidedBy ?? initiatorDeviceId,
      poolAfterCny: poolAfter,
      ts: new Date(now()).toISOString(),
    };
    appendPoolSpend(dirs, record.groupId, entry);
    record.pool.pendingCny = round2Cny(record.pool.pendingCny - amountCny);
    record.pool.spentCny = poolAfter;
    member.spentCny = round2Cny(member.spentCny + amountCny);
    persist(record);
    broadcast({ type: "group.pool-spend", session_id: record.groupId, poolSpend: entry });
  }

  // ── member turn execution (the §K pipeline posture, re-keyed) ────────────

  function engineClientFor(sessionKey: string): EngineWsClient {
    return (
      opts.engineClientFactory ??
      ((key: string) =>
        // Same construction the dispatch pipeline's lane client performs.
        new EngineWsClient({
          url: opts.engineWsUrl,
          model: opts.defaultModel ?? null,
          sessionId: key,
          headers: opts.engineAuthToken
            ? { authorization: `Bearer ${opts.engineAuthToken}` }
            : undefined,
        }))
    )(sessionKey);
  }

  // ── B6.2: the create-time crew planning query (提案开放问题 3 收尾) ────────
  // One engine query, host-initiated (gateway 自调用): no phone session in the
  // loop, no tool turns, no approval face. Any failure — transport, timeout
  // (default 30s), unparseable/illegal output — degrades honestly to the
  // generic 3-slot template and the create response returns the template
  // members verbatim (what you see is what was planned).

  function planningPrompt(goal: string): string {
    return [
      `[群] 目标：${goal}`,
      "推荐 2-4 人分工阵容，输出严格 JSON 数组 [{slot,label,title}]，不要输出数组以外的任何文字。",
    ].join("\n");
  }

  /** Strict parse of the planned crew: a JSON array of 2–4 unique
   *  slot/label/title triples. Anything else → null (template fallback). */
  function parseCrewJson(text: string): Array<Pick<GroupMember, "slot" | "label" | "title">> | null {
    const start = text.indexOf("[");
    const end = text.lastIndexOf("]");
    if (start < 0 || end <= start) return null;
    let parsed: unknown;
    try {
      parsed = JSON.parse(text.slice(start, end + 1));
    } catch {
      return null;
    }
    if (!Array.isArray(parsed) || parsed.length < 2 || parsed.length > 4) return null;
    const slots = new Set<string>();
    const labels = new Set<string>();
    const crew: Array<Pick<GroupMember, "slot" | "label" | "title">> = [];
    for (const item of parsed) {
      const rec = (item ?? {}) as Record<string, unknown>;
      const slot = typeof rec.slot === "string" ? rec.slot.trim() : "";
      const label = typeof rec.label === "string" ? rec.label.trim() : "";
      const title = typeof rec.title === "string" ? rec.title.trim() : "";
      if (!slot || !label || !title || slots.has(slot) || labels.has(label)) return null;
      slots.add(slot);
      labels.add(label);
      crew.push({ slot, label, title });
    }
    return crew;
  }

  /** One planning query under a one-shot UUID session. Returns the parsed
   *  crew, or null on ANY failure (the caller falls back to the template). */
  async function planCrewFromEngine(
    goal: string,
  ): Promise<Array<Pick<GroupMember, "slot" | "label" | "title">> | null> {
    const client = engineClientFor(`group:plan:${randomUUID()}`);
    const chunks: string[] = [];
    const consume = async (): Promise<string> => {
      await client.connect();
      // One-shot UUID session: the engine WS gate rejects non-UUID ids, and
      // no phone-visible session key ever enters the prompt.
      const query = client.runQuery(planningPrompt(goal), {
        sessionId: randomUUID(),
      }) as AsyncIterable<EngineEvent>;
      for await (const ev of query) {
        if (ev.type === "text") chunks.push(ev.content);
        else if (ev.type === "failed") throw new Error(ev.error);
      }
      return chunks.join("");
    };
    let timeoutTimer: ReturnType<typeof setTimeout> | null = null;
    try {
      const text = await Promise.race([
        consume(),
        new Promise<never>((_, reject) => {
          timeoutTimer = setTimeout(
            () => reject(new Error("crew planning query timed out")),
            opts.planningTimeoutMs ?? DEFAULT_PLANNING_TIMEOUT_MS,
          );
          timeoutTimer.unref?.();
        }),
      ]);
      const crew = parseCrewJson(text);
      if (!crew) opts.logger.warn("group.create: crew planning output unusable → template fallback");
      return crew;
    } catch (err) {
      opts.logger.warn(`group.create: crew planning failed → template fallback: ${(err as Error).message}`);
      return null;
    } finally {
      if (timeoutTimer !== null) clearTimeout(timeoutTimer);
      await client.close().catch(() => {});
    }
  }

  function composePrompt(record: GroupRecord, member: GroupMember, userText: string | null, handoffNote: string | null): string {
    const lines = [
      `[群] 目标：${record.goal}`,
      `[你的分工] ${member.title}（${member.slot}）· 信息边界（${record.rules.infoBoundary}）：只使用完成该分工所需的信息`,
    ];
    if (handoffNote) lines.push(`[上一棒交接] ${handoffNote}`);
    if (userText) lines.push(`[用户消息] ${userText}`);
    return lines.join("\n");
  }

  /** One member turn: engine query under a one-shot UUID session, streamed
   *  onto the group thread (session_id = groupId), approvals through the
   *  shared hub round-trip (with §S attribution), transcript + status kept
   *  honest. Returns the final text (or null on failure). */
  async function runMemberTurn(
    record: GroupRecord,
    member: GroupMember,
    userText: string | null,
    handoffNote: string | null,
    initiatorDeviceId: string,
  ): Promise<string | null> {
    member.status = "working";
    member.statusNote = `正在完成分工：${member.title}`;
    memberStatusEvent(record, member);
    persist(record);

    const client = engineClientFor(`group:${record.groupId}:${member.memberId}:${randomUUID()}`);
    const engineSessionId = randomUUID(); // one-shot; never a group key (engine UUID gate)
    const acc: { chunks: string[]; failed: string | null; cancelled: boolean } = {
      chunks: [],
      failed: null,
      cancelled: false,
    };
    try {
      await client.connect();
      const query = client.runQuery(composePrompt(record, member, userText, handoffNote), {
        sessionId: engineSessionId,
      }) as AsyncIterable<EngineEvent>;
      for await (const ev of query) {
        switch (ev.type) {
          case "text":
            broadcast({ type: "task.progress", session_id: record.groupId, content: ev.content });
            acc.chunks.push(ev.content);
            break;
          case "usage":
            broadcast({
              type: "task.progress",
              session_id: record.groupId,
              usage: {
                input_tokens: ev.input_tokens,
                output_tokens: ev.output_tokens,
                cost_usd: ev.cost_usd,
              },
            });
            break;
          case "tool_use":
          case "tool_result": {
            const frame =
              ev.type === "tool_use"
                ? {
                    kind: "use" as const,
                    name: ev.name,
                    input: ev.input,
                    ...(typeof ev.tool_use_id === "string" && ev.tool_use_id.length > 0
                      ? { tool_use_id: ev.tool_use_id }
                      : {}),
                    ...(typeof ev.ts === "number" && Number.isFinite(ev.ts) ? { ts: ev.ts } : {}),
                  }
                : {
                    kind: "result" as const,
                    name: ev.name,
                    output: ev.output,
                    ...(typeof ev.tool_use_id === "string" && ev.tool_use_id.length > 0
                      ? { tool_use_id: ev.tool_use_id }
                      : {}),
                    ...(typeof ev.is_error === "boolean" ? { is_error: ev.is_error } : {}),
                    ...(ev.meta != null ? { meta: ev.meta } : {}),
                    ...(typeof ev.ts === "number" && Number.isFinite(ev.ts) ? { ts: ev.ts } : {}),
                  };
            broadcast({ type: "task.progress", session_id: record.groupId, tool: frame });
            break;
          }
          case "approval_request": {
            member.status = "waiting-approval";
            memberStatusEvent(record, member);
            persist(record);
            // ── §S B6.1: the honest amount 口径, computed BEFORE the ask.
            // An unresolvable amount (no amountCny/value in the tool input)
            // bypasses the entire pool path: no pending, no ledger, no
            // escalation — 宁缺勿造.
            const amountCny = resolvePoolSpendAmountCny(ev.tool_input);
            // The R5 one-shot: after a handoff, this member's next ask names
            // the trigger, then the flag clears (the red line fires once).
            // A pool escalation names its own trigger when the amount is
            // resolvable — but AT MOST ONE key rides per ask: handoff-first
            // wins when both hit (the over-* verdict is still settled below,
            // only the label stays single).
            const over = overRuleTrigger(record, member, amountCny);
            const ruleTrigger =
              member.handoffFirstPending === true ? ("handoff-first" as const) : over;
            member.handoffFirstPending = false;
            // §S B6.0-4 quoteWindow seat: a payments-connector quote would
            // set BOTH the ask's per-ask gateway deadline (`deadlineMs`) AND
            // the engine query's `approval_ttl_ms` to the same window so the
            // two sides never disagree. v1 has NO quoteWindow producer (no
            // payment-class tool — 裁决), so both stay absent and the group
            // ask keeps the exact legacy 300s window everywhere. The
            // mechanism (hub deadline + registry retention + engine frame
            // key) is live underneath this seam.
            const quoteWindowMs: number | null = null;
            // An amount-bearing ask enters the pool's pending column the
            // moment it goes out (persisted) — allow converts it into
            // ledger + spent, deny/expiry refunds it.
            if (amountCny !== null) {
              record.pool.pendingCny = round2Cny(record.pool.pendingCny + amountCny);
              persist(record);
            }
            const settled = await hub.requestApprovalWithMeta(initiatorDeviceId, {
              requestId: ev.request_id,
              toolName: ev.tool_name,
              toolInput: ev.tool_input,
              description: ev.description,
              isDestructive: ev.is_destructive,
              diffPreview: ev.diff_preview ?? null,
              ...(typeof ev.ts === "number" && ev.ts !== null ? { ts: ev.ts } : {}),
              ...(ev.agent ? { agent: ev.agent } : {}),
              ...(ev.risk ? { risk: ev.risk } : {}),
              ...(quoteWindowMs != null ? { deadlineMs: quoteWindowMs } : {}),
              group: {
                groupId: record.groupId,
                member: {
                  memberId: member.memberId,
                  label: member.label,
                  title: member.title,
                  source: member.source,
                },
                ...(ruleTrigger ? { ruleTrigger } : {}),
                // §S B6.1: rides only when the amount resolved. With the
                // pending bump already applied above, `total − spent −
                // pending` IS `total − spent − pendingBefore − amount`
                // (remainingAfter), negative when the ask would overdraw —
                // shipped as-is, never clamped.
                ...(amountCny !== null
                  ? {
                      poolAfter: {
                        poolCny: record.pool.totalCny,
                        remainingAfterCny: round2Cny(
                          record.pool.totalCny - record.pool.spentCny - record.pool.pendingCny,
                        ),
                      },
                    }
                  : {}),
              },
            });
            // B6.1 settlement: allow → ledger + broadcast + pool/member
            // numbers; deny/expiry → pending refunds, ledger untouched. A
            // no-amount ask bypasses the pool entirely (default path).
            if (amountCny !== null) {
              settlePoolSpend(record, member, ev, settled, amountCny, initiatorDeviceId);
            }
            // Forward the decision so the engine resumes (mirror the IM/task
            // handler; a failed POST degrades to the engine's own timeout-deny).
            await respondToApproval({
              engineBaseUrl: opts.engineHttpBaseUrl,
              requestId: ev.request_id,
              choice: settled.choice,
              authToken: opts.engineAuthToken ?? null,
              fetchImpl: opts.fetchImpl,
            }).catch((err) => {
              opts.logger.warn(
                `group turn: approval POST failed (engine will deny at its window): ${(err as Error).message}`,
              );
            });
            member.status = "working";
            member.statusNote = `正在完成分工：${member.title}`;
            memberStatusEvent(record, member);
            break;
          }
          case "failed":
            acc.failed = ev.error;
            break;
          case "cancelled":
            acc.cancelled = true;
            break;
          default:
            break;
        }
      }
    } catch (err) {
      acc.failed = (err as Error).message;
    } finally {
      await client.close().catch(() => {});
    }

    const finalText = acc.chunks.join("");
    if (acc.failed !== null || acc.cancelled || finalText.length === 0) {
      member.status = "failed";
      member.statusNote = acc.failed ?? "任务取消";
      memberStatusEvent(record, member);
      persist(record);
      systemEvent(
        record,
        acc.cancelled ? "member-failed" : "member-failed",
        `成员「${member.title}（${member.label}）」的任务未完成${acc.failed ? `：${acc.failed}` : "。"}`
      );
      return null;
    }
    member.status = "done";
    member.statusNote = null;
    memberStatusEvent(record, member);
    transcriptAdd(record, {
      role: "assistant",
      content: finalText,
      kind: "member",
      member: { memberId: member.memberId, label: member.label, title: member.title },
    });
    // §K3 terminal parity: the member turn closes its streaming phase with
    // `task.message {session_id: groupId}` so the phone's existing routing
    // chain seals the bubble with the final full text, zero changes.
    broadcast({ type: "task.message", session_id: record.groupId, text: finalText });
    persist(record);
    return finalText;
  }

  /** The deterministic handoff chain: run `start`, then walk plan order
   *  handing off to the next non-finished member until the plan exhausts
   *  (group-completed) or a member fails (chain stops, honestly). */
  async function runChain(
    record: GroupRecord,
    start: GroupMember,
    userText: string | null,
    initiatorDeviceId: string,
  ): Promise<void> {
    let current = start;
    let text: string | null = null;
    let note: string | null = null;
    let first = true;
    for (;;) {
      const produced = await runMemberTurn(record, current, first ? userText : null, note, initiatorDeviceId);
      first = false;
      if (produced === null) return; // member-failed card already out; chain stops
      text = produced;
      const next = record.members.find(
        (m) => m.status !== "done" && m.status !== "archived" && m.status !== "failed",
      );
      if (!next) {
        record.status = "completed";
        persist(record);
        systemEvent(
          record,
          "group-completed",
          `群目标已由全体成员完成，共 ${record.members.length} 个分工。`
        );
        return;
      }
      // Handoff (R5): confirmation-free by the locked rule, but the receiver
      // owes one explicit ask before its first outward payment — modeled as
      // the one-shot handoff-first flag.
      next.handoffFirstPending = true;
      next.status = "queued";
      const handoffText = `「${current.title}（${current.label}）」已交接给「${next.title}（${next.label}）」：${clip(text, 280)}`;
      broadcast({
        type: "group.handoff",
        session_id: record.groupId,
        handoff: {
          id: `ho-${randomUUID().slice(0, 8)}`,
          from: current.memberId,
          to: next.memberId,
          ts: new Date(now()).toISOString(),
          note: clip(text, 280),
        },
      });
      systemEvent(record, "handoff", handoffText);
      current = next;
      note = clip(text, 280);
    }
  }

  function clip(text: string, max: number): string {
    return text.length <= max ? text : `${text.slice(0, max)}…`;
  }

  function runExclusive(groupId: string, task: () => Promise<void>): Promise<void> {
    const prev = chains.get(groupId) ?? Promise.resolve();
    const nextTask = prev.then(task, task);
    chains.set(
      groupId,
      nextTask.catch(() => {}),
    );
    return nextTask;
  }

  // ── method handlers ──────────────────────────────────────────────────────

  const groupGate = (ctx: MethodContext) => {
    if (ctx.sessionId == null) return PAIRING_REQUIRED;
    return null;
  };

  const handlers: MethodHandlers = {
    // ── §S B6.0-1: group list (the 02 "团队" projection; double source with
    // session.list — the two never mix). Capability note: an old gateway
    // answers METHOD_NOT_FOUND, which is exactly the phone's honest signal.
    "shannon/group.list": async (_raw, ctx) => {
      const gate = groupGate(ctx);
      if (gate) return gate;
      const groups: GroupListItem[] = listGroups(dirs)
        .sort((a, b) => (a.lastActivityAt < b.lastActivityAt ? 1 : -1))
        .map((record) => ({
          groupId: record.groupId,
          title: record.title,
          goalSummary: record.goal.length > 0 ? clip(record.goal, 60) : undefined,
          status: record.status,
          memberCount: record.members.filter((m) => m.status !== "archived").length,
          pool: {
            totalCny: record.pool.totalCny,
            spentCny: record.pool.spentCny,
            pendingCny: record.pool.pendingCny,
          },
          lastActivityAt: record.lastActivityAt,
        }));
      return { kind: "result", result: { groups } satisfies GroupListResult };
    },

    // ── §S B6.2 收尾面: one group entity by id. Unknown groupId is
    // INVALID_PARAMS (the §J2 honest miss). Archived/completed groups carry
    // the persisted `review` when one exists (honest absence otherwise —
    // see [GroupObject.review]).
    "shannon/group.get": async (raw, ctx) => {
      const gate = groupGate(ctx);
      if (gate) return gate;
      const params = (raw ?? {}) as Partial<GroupGetParams>;
      if (typeof params.groupId !== "string" || !GROUP_ID_RE.test(params.groupId)) {
        return { kind: "error", code: ShannonError.BAD_PARAMS, message: "params.groupId (grp-<uuid>) is required" };
      }
      const record = loadGroup(dirs, params.groupId);
      if (!record) {
        return { kind: "error", code: ShannonError.BAD_PARAMS, message: "unknown groupId" };
      }
      return { kind: "result", result: { group: groupWire(record) } satisfies GroupGetResult };
    },

    // ── §S B6.0-2: dual-path create. R12's step-2 confirm sheet is PHONE-
    // LOCAL — one round trip here; the user's tap on 「建群，出发」 is what
    // produces this call (no half-created groups: validation precedes any
    // filesystem write).
    "shannon/group.create": async (raw, ctx) => {
      const gate = groupGate(ctx);
      if (gate) return gate;
      const params = (raw ?? {}) as Partial<GroupCreateParams>;
      if (typeof params.goal !== "string" || params.goal.trim().length === 0) {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: "params.goal (non-empty string) is required",
        };
      }
      const path = params.path ?? "ephemeral";
      if (path !== "ephemeral" && path !== "roster") {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: 'params.path must be "ephemeral" or "roster"',
        };
      }
      // The locked rule (14): payments always ask. Absent = true; an explicit
      // false is rejected, never silently stored.
      const rules: GroupRules = {
        paymentsAskFirst: true,
        infoBoundary:
          typeof params.rules?.infoBoundary === "string" && params.rules.infoBoundary.length > 0
            ? params.rules.infoBoundary
            : "need-only",
        handoffFree: params.rules?.handoffFree !== false,
        dailyReportAt:
          typeof params.rules?.dailyReportAt === "string" && /^\d{2}:\d{2}$/.test(params.rules.dailyReportAt)
            ? params.rules.dailyReportAt
            : null,
      };
      // Runtime check regardless of the wire type's `true` literal — the
      // locked rule must REJECT an explicit false, never store it.
      if ((params.rules as { paymentsAskFirst?: unknown } | undefined)?.paymentsAskFirst === false) {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: "rules.paymentsAskFirst is locked to true (payments always ask first)",
        };
      }
      const totalCny = params.pool?.totalCny == null ? 0 : cny(params.pool.totalCny);
      if (totalCny === null) {
        return { kind: "error", code: ShannonError.BAD_PARAMS, message: "pool.totalCny must be a non-negative number (元)" };
      }
      // B6.2: an ephemeral create WITHOUT members runs the one-shot engine
      // planning query first (roster path never plans — its members must
      // carry agentIds, which the template cannot). Failure degrades to the
      // template; either way the response returns the actual members.
      const planned =
        params.members == null && path === "ephemeral"
          ? await planCrewFromEngine(params.goal.trim())
          : null;
      const requested: NonNullable<GroupCreateParams["members"]> =
        planned ?? (params.members ?? templateMembers().map((m) => ({ ...m })));
      if (!Array.isArray(requested) || requested.length === 0 || requested.length > 8) {
        return { kind: "error", code: ShannonError.BAD_PARAMS, message: "members must be a non-empty array (≤8)" };
      }
      const seenSlots = new Set<string>();
      const seenLabels = new Set<string>();
      let shares = 0;
      const members: GroupMember[] = [];
      for (const m of requested) {
        if (
          typeof m?.slot !== "string" || m.slot.length === 0 ||
          typeof m?.label !== "string" || m.label.length === 0 ||
          typeof m?.title !== "string" || m.title.length === 0
        ) {
          return { kind: "error", code: ShannonError.BAD_PARAMS, message: "each member needs non-empty slot/label/title" };
        }
        if (seenSlots.has(m.slot) || seenLabels.has(m.label)) {
          return { kind: "error", code: ShannonError.BAD_PARAMS, message: "member slot/label must be unique" };
        }
        seenSlots.add(m.slot);
        seenLabels.add(m.label);
        const shareCny = m.shareCny == null ? 0 : cny(m.shareCny);
        if (shareCny === null) {
          return { kind: "error", code: ShannonError.BAD_PARAMS, message: "shareCny must be a non-negative number (元)" };
        }
        shares += shareCny;
        let agentId: string | null = null;
        if (path === "roster") {
          // §K1 posture: an unknown roster id is INVALID_PARAMS, never a
          // silent re-route (attribution is the contract, not a hint).
          if (typeof m.agentId !== "string" || m.agentId.length === 0) {
            return { kind: "error", code: ShannonError.BAD_PARAMS, message: "roster path: every member needs an agentId" };
          }
          const roster = loadAgentRoster(opts.agentRosterDirs);
          if (!roster.some((a) => a.id === m.agentId)) {
            return {
              kind: "error",
              code: ShannonError.BAD_PARAMS,
              message: "unknown agentId — not in this host's agent roster (shannon/agent.list)",
              data: { agentId: m.agentId },
            };
          }
          agentId = m.agentId;
        } else if (typeof m.agentId === "string" && m.agentId.length > 0) {
          return { kind: "error", code: ShannonError.BAD_PARAMS, message: "ephemeral members cannot carry an agentId" };
        }
        members.push({
          memberId: `mem-${String(members.length + 1).padStart(2, "0")}`,
          slot: m.slot,
          label: m.label,
          title: m.title,
          source: path === "roster" ? "roster" : "ephemeral",
          agentId,
          shareCny,
          spentCny: 0,
          status: "queued",
          statusNote: null,
          permissions: Array.isArray(m.permissions)
            ? m.permissions.filter((p): p is string => typeof p === "string")
            : ["read.only"],
        });
      }
      if (shares > totalCny) {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: "Σ shareCny exceeds pool.totalCny (the reserve is total − Σ shares)",
        };
      }
      const record: GroupRecord = {
        groupId: newGroupId(),
        title: clip(params.goal.trim(), 24),
        goal: params.goal.trim(),
        status: "active",
        createdAt: isoNow(opts),
        lastActivityAt: isoNow(opts),
        path,
        rules,
        pool: {
          totalCny,
          spentCny: 0,
          pendingCny: 0,
          reserveCny: Math.round((totalCny - shares) * 100) / 100,
        },
        members,
      };
      saveGroup(dirs, record);
      transcriptAdd(record, {
        role: "assistant",
        content: `群已建立 · 目标：${record.goal} · 池 ¥${record.pool.totalCny.toFixed(2)}（备用 ¥${record.pool.reserveCny.toFixed(2)}）· ${members.length} 个成员分工已就绪`,
        kind: "system",
        systemKind: "group-created",
      });
      // B6.2: a newly configured dailyReportAt must start being served
      // without a restart (arm() no-ops when nothing is eligible).
      reports.arm();
      opts.logger.info(
        `shannon/group.create: ${record.groupId} (${path}, ${members.length} members${planned ? ", planned crew" : ""}) by ${ctx.sessionId}`,
      );
      return { kind: "result", result: { group: groupWire(record) } satisfies GroupCreateResult };
    },

    // ── §S B6.0-2/3: speak into the group → deterministic member turn(s).
    // Response is synchronous ({messageId, ts}); the member stream flows as
    // events with session_id = groupId.
    "shannon/group.message": async (raw, ctx) => {
      const gate = groupGate(ctx);
      if (gate) return gate;
      const params = (raw ?? {}) as Partial<GroupMessageParams>;
      if (typeof params.groupId !== "string" || !GROUP_ID_RE.test(params.groupId)) {
        return { kind: "error", code: ShannonError.BAD_PARAMS, message: "params.groupId (grp-<uuid>) is required" };
      }
      if (typeof params.text !== "string" || params.text.trim().length === 0) {
        return { kind: "error", code: ShannonError.BAD_PARAMS, message: "params.text (non-empty string) is required" };
      }
      const record = loadGroup(dirs, params.groupId);
      if (!record) {
        // §J2 posture for unknown group keys: an honest miss the phone can
        // render, not a fabricated thread.
        return { kind: "error", code: ShannonError.BAD_PARAMS, message: "unknown groupId" };
      }
      if (record.status !== "active") {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: `group is ${record.status} — messaging needs an active group`,
        };
      }
      const text = params.text.trim();
      const ts = new Date(now()).toISOString();
      transcriptAdd(record, { role: "user", content: text, kind: "user" });
      persist(record);

      // Routing: mention wins; otherwise the first member still owed work in
      // plan order. Nothing is invented — an exhausted plan answers with the
      // honest system card instead of a phantom turn.
      let target: GroupMember | undefined;
      if (typeof params.mentionMemberId === "string" && params.mentionMemberId.length > 0) {
        target = record.members.find((m) => m.memberId === params.mentionMemberId);
        if (!target) {
          return { kind: "error", code: ShannonError.BAD_PARAMS, message: "unknown mentionMemberId" };
        }
        if (target.status === "archived") {
          return { kind: "error", code: ShannonError.BAD_PARAMS, message: "mentioned member is archived" };
        }
      } else {
        target = record.members.find(
          (m) => m.status !== "done" && m.status !== "archived" && m.status !== "failed",
        );
        if (!target) {
          systemEvent(record, "group-completed", "全体成员的分工已完成——没有待办的成员任务。");
          return { kind: "result", result: { messageId: `msg-${randomUUID().slice(0, 8)}`, ts } satisfies GroupMessageResult };
        }
      }

      const initiator = ctx.sessionId as string;
      const started = target;
      const userText = text;
      void runExclusive(record.groupId, async () => {
        try {
          await runChain(record, started, userText, initiator);
        } catch (err) {
          opts.logger.warn(`group chain crashed: ${(err as Error).message}`);
          systemEvent(record, "member-failed", `群编排异常：${(err as Error).message}`);
        }
      });
      return {
        kind: "result",
        result: { messageId: `msg-${randomUUID().slice(0, 8)}`, ts } satisfies GroupMessageResult,
      };
    },

    // ── §S B6.0-5: archive/dissolve (15/16). Transcript is kept for replay.
    "shannon/group.archive": async (raw, ctx) => {
      const gate = groupGate(ctx);
      if (gate) return gate;
      const params = (raw ?? {}) as Partial<GroupArchiveParams>;
      if (typeof params.groupId !== "string" || !GROUP_ID_RE.test(params.groupId)) {
        return { kind: "error", code: ShannonError.BAD_PARAMS, message: "params.groupId (grp-<uuid>) is required" };
      }
      if (params.reason !== "completed" && params.reason !== "disbanded") {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: 'params.reason must be "completed" or "disbanded"',
        };
      }
      const record = loadGroup(dirs, params.groupId);
      if (!record) {
        return { kind: "error", code: ShannonError.BAD_PARAMS, message: "unknown groupId" };
      }
      // B6.2: aggregate the 16-screen review from the AS-LIVED state BEFORE
      // the flips below (goals.done counts members that actually finished).
      // Aggregation failure degrades to review-absent — the archive itself
      // never fails on it (honest absence, not a broken close).
      const review = params.reason === "completed" ? buildReview(record) : null;
      record.status = params.reason === "completed" ? "completed" : "archived";
      for (const m of record.members) {
        if (m.status !== "failed") m.status = "archived";
        m.statusNote = null;
      }
      if (review) record.review = review;
      persist(record);
      systemEvent(
        record,
        "group-archived",
        params.reason === "completed"
          ? "群已归档：已完成的部分保留在转录里，可随时回看。"
          : "群已解散：已完成的部分先归档，转录保留可回看。"
      );
      opts.logger.info(`shannon/group.archive: ${record.groupId} → ${record.status}`);
      // B6.2: the tick chain may need to stand down (no eligible group left).
      reports.arm();
      return {
        kind: "result",
        result: { ok: true, ...(review ? { review } : {}) } satisfies GroupArchiveResult,
      };
    },
  };

  return {
    handlers,
    stop() {
      reports.stop();
    },
  };
}

// ── §J2 interception helper (wired into shannon/session.history) ────────────

/**
 * The `session.history` interception for group keys (B6.0-1): a `grp-*`
 * sessionId answers from the group transcript store in the exact §J2 wire
 * shape; anything else (or a miss) returns null so the caller falls through
 * to the engine unchanged. Injected via `EngineBridgeOptions.groupHistoryLookup`.
 */
export function groupHistoryLookup(
  groupsDirs: string[] | undefined,
): (sessionId: string, paging: { before?: string; limit?: number }) => MobileTranscriptWire | null {
  const dirs = groupsDirs ?? [];
  return (sessionId, paging) => {
    if (!sessionId.startsWith("grp-")) return null;
    const record = loadGroup(dirs, sessionId);
    if (!record) return null;
    const page = pageTranscript(readTranscript(dirs, sessionId), paging);
    return {
      sessionId,
      messages: page.entries.map((e) => {
        const message: MobileTranscriptWire["messages"][number] = {
          role: e.role,
          content: e.content,
          ...(e.ts ? { ts: e.ts } : {}),
        };
        return message;
      }),
      hasMore: page.hasMore,
    };
  };
}
