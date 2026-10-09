/**
 * §S group orchestration (B6.0 起步 + B6.2 收尾) — the `shannon/group.*` face
 * + the deterministic member-turn orchestrator, against a real
 * MobileDispatchHub, a tmp group store and a seam-routed fake engine. Pins:
 *
 *  - create validation matrix (locked payments rule / roster attribution /
 *    share-over-pool / ephemeral agentId ban) — no half-created groups;
 *  - the B6.2 crew planning query (ephemeral default members → one engine
 *    query; failure/timeout/unusable output → the honest template degrade);
 *  - the group-thread event stream (session_id = groupId, broadcast to ALL
 *    connected devices — a group is host-level, unlike §K3 task streams);
 *  - the deterministic handoff chain (plan order → handoff card → next
 *    member → group-completed; failure stops the chain honestly);
 *  - the approval round-trip with §S attribution (group key, one-shot
 *    handoff-first trigger) riding the shared hub settle;
 *  - §J2 interception (groupHistoryLookup) incl. paging + engine fallback;
 *  - archive lifecycle (archived group rejects messages, transcript kept);
 *  - B6.2 收尾: `group.get` + the persisted review aggregate (16 屏, honest
 *    0s where v1 has no counting face) and the daily-report tick (R11 —
 *    fires at the local HH:mm, dedupes per day, stands down after archive,
 *    stops with the host).
 */

import { EventEmitter } from "node:events";
import { mkdtempSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { WebSocket } from "ws";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Logger } from "../../adapters/types.js";
import type { EngineEvent } from "../../engine/runtime.js";
import type { EngineWsClient } from "../../engine/wsClient.js";
import { createConsoleLogger } from "../../logger.js";
import { ApprovalRegistry } from "../approvalRegistry.js";
import { MobileDispatchHub } from "../hub.js";
import {
  createGroupHandlers,
  groupHistoryLookup,
  type GroupHandlersBundle,
} from "../groupHandlers.js";
import { loadGroup, readPoolLedger, readTranscript, saveGroup } from "../groupStore.js";
import { localDateKey } from "../groupReview.js";
import type { MethodContext } from "../server.js";

const logger: Logger = createConsoleLogger("error");

class FakeDeviceSocket extends EventEmitter {
  readyState = WebSocket.OPEN;
  frames: any[] = [];

  send(data: string): void {
    this.frames.push(JSON.parse(data));
  }
}

function fakeCtx(deviceId: string | null, socket = new FakeDeviceSocket()): MethodContext {
  return { socket, sessionId: deviceId, logger } as unknown as MethodContext;
}

function eventsOf(ctx: MethodContext): any[] {
  return ((ctx.socket as unknown as FakeDeviceSocket).frames as any[])
    .filter((f) => f.method === "shannon/event")
    .map((f) => f.params);
}

function mockEngineClient(events: EngineEvent[]): EngineWsClient {
  return {
    connect: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
    cancel: vi.fn(() => {}),
    runQuery: vi.fn(async function* (): AsyncGenerator<EngineEvent> {
      for (const e of events) yield e;
    }),
  } as unknown as EngineWsClient;
}

/** A client whose query never yields (the planning-timeout degrade fixture). */
function hangingEngineClient(): EngineWsClient {
  return {
    connect: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
    cancel: vi.fn(() => {}),
    runQuery: vi.fn(() =>
      (async function* (): AsyncGenerator<EngineEvent> {
        await new Promise<never>(() => {});
      })(),
    ),
  } as unknown as EngineWsClient;
}

function textEvent(content: string): EngineEvent {
  return { type: "text", content } as EngineEvent;
}

interface Harness {
  hub: MobileDispatchHub;
  facet: GroupHandlersBundle;
  handlers: GroupHandlersBundle["handlers"];
  groupsDirs: string[];
  clients: EngineWsClient[];
  /** The hub's pending-approval registry (exposed for restore-face asserts). */
  approvals: ApprovalRegistry;
  initiator: MethodContext;
  bystander: MethodContext;
}

function buildHarness(opts: {
  /** One script PER engine client — planning and member turns each build one. */
  turnScripts?: EngineEvent[][];
  /** One roster dir injected as the create-path validation target. */
  agentRoster?: string;
  fetchImpl?: typeof fetch;
  /** Hub-wide approval parking window (default 300s; tests shrink it). */
  approvalTimeoutMs?: number;
  /** B6.0-4: the group quote window (default 30min; tests shrink it). */
  quoteWindowMs?: number;
  now?: () => number;
  planningTimeoutMs?: number;
  reportScheduleNext?: (fn: () => void, delayMs: number) => () => void;
} = {}): Harness {
  const groupsDirs = [mkdtempSync(join(tmpdir(), "shannon-groups-test-"))];
  const approvals = new ApprovalRegistry();
  const hub = new MobileDispatchHub({
    logger,
    approvals,
    ...(opts.approvalTimeoutMs !== undefined ? { approvalTimeoutMs: opts.approvalTimeoutMs } : {}),
  });
  const clients: EngineWsClient[] = [];
  const turnScripts = [...(opts.turnScripts ?? [])];
  // A tick clock: distinct ms per mutation so §J2 same-ts pagination groups
  // never fuse entries inside a fast test.
  let tick = 1_770_000_000_000;
  const facet = createGroupHandlers({
    hub,
    logger,
    now: opts.now ?? (() => (tick += 1000)),
    groupsDirs,
    agentRosterDirs: opts.agentRoster ? [opts.agentRoster] : [],
    engineWsUrl: "ws://engine:33420/api/ws",
    engineHttpBaseUrl: "http://engine:33420",
    planningTimeoutMs: opts.planningTimeoutMs,
    reportScheduleNext: opts.reportScheduleNext,
    ...(opts.quoteWindowMs !== undefined ? { quoteWindowMs: opts.quoteWindowMs } : {}),
    engineClientFactory: () => {
      const client = mockEngineClient(turnScripts.shift() ?? []);
      clients.push(client);
      return client;
    },
    fetchImpl: opts.fetchImpl,
  });
  const initiator = fakeCtx("dev-1");
  const bystander = fakeCtx("dev-2");
  hub.registerConnection(initiator);
  hub.registerConnection(bystander);
  return {
    hub,
    facet,
    handlers: facet.handlers,
    groupsDirs,
    clients,
    approvals,
    initiator,
    bystander,
  };
}

async function createGroup(
  h: Harness,
  members?: unknown,
  overrides: Record<string, unknown> = {},
): Promise<any> {
  const res: any = await h.handlers["shannon/group.create"]!(
    {
      goal: "东京行程筹备：机票 + 酒店 + 签证材料清单",
      pool: { totalCny: 8000 },
      ...(members !== undefined ? { members } : {}),
      ...overrides,
    },
    h.initiator,
  );
  return res;
}

const TWO_MEMBER_PLAN = [
  { slot: "flights", label: "A", title: "订机票" },
  { slot: "hotels", label: "M", title: "订酒店" },
];

describe("shannon/group.create (§S B6.0-2)", () => {
  it("unpaired session → PAIRING_REQUIRED before anything else", async () => {
    const h = buildHarness();
    const res: any = await h.handlers["shannon/group.create"]!({ goal: "x" }, fakeCtx(null));
    expect(res.kind).toBe("error");
    expect(res.code).toBe(-32000);
  });

  it("ephemeral create (default path): grp- id, queued members, reserve = total − Σ shares", async () => {
    const h = buildHarness();
    const res = await createGroup(h, TWO_MEMBER_PLAN.map((m) => ({ ...m, shareCny: 3600 })));
    expect(res.kind).toBe("result");
    const group = res.result.group;
    expect(group.groupId).toMatch(/^grp-[0-9a-f-]{36}$/);
    expect(group.status).toBe("active");
    expect(group.rules).toMatchObject({ paymentsAskFirst: true, handoffFree: true });
    expect(group.pool).toEqual({ totalCny: 8000, spentCny: 0, pendingCny: 0, reserveCny: 800 });
    expect(group.members).toHaveLength(2);
    expect(group.members[0]).toMatchObject({
      memberId: "mem-01",
      source: "ephemeral",
      agentId: null,
      status: "queued",
    });
  });

  it("members omitted → the generic template crew plans (honest, no invented agentIds)", async () => {
    const h = buildHarness();
    const res = await createGroup(h, undefined);
    expect(res.kind).toBe("result");
    expect(res.result.group.members).toHaveLength(3);
    for (const m of res.result.group.members) {
      expect(m.source).toBe("ephemeral");
      expect(m.agentId).toBeNull();
    }
  });

  it("validation matrix: each bad shape is INVALID_PARAMS with no group written", async () => {
    const h = buildHarness();
    const cases: Array<[Record<string, unknown>, string]> = [
      [{ goal: "" }, "goal"],
      [{ path: "surprise" }, "path"],
      [{ rules: { paymentsAskFirst: false } }, "locked"],
      [{ pool: { totalCny: -1 } }, "totalCny"],
      [{ members: [] }, "members"],
    ];
    for (const [overrides, label] of cases) {
      const res = await createGroup(h, TWO_MEMBER_PLAN, overrides);
      expect(res.kind, label).toBe("error");
      expect(res.code, label).toBe(-32001);
    }
    // Σ shares > pool → rejected.
    const over = await createGroup(
      h,
      TWO_MEMBER_PLAN.map((m) => ({ ...m, shareCny: 5000 })),
    );
    expect(over.kind).toBe("error");
    // Ephemeral members cannot carry an agentId.
    const ghost = await createGroup(h, [{ slot: "s", label: "A", title: "t", agentId: "nope" }]);
    expect(ghost.kind).toBe("error");
    // Nothing persisted by any rejected call.
    const list: any = await h.handlers["shannon/group.list"]!({}, h.initiator);
    expect(list.result.groups).toHaveLength(0);
  });

  it("roster path: unknown agentId → INVALID_PARAMS (§K1 不静默转投); roster hit attributed", async () => {
    const rosterDir = mkdtempSync(join(tmpdir(), "shannon-roster-test-"));
    const { writeFileSync } = await import("node:fs");
    writeFileSync(
      join(rosterDir, "api-refactor.toml"),
      'name = "api-refactor"\ndescription = "refactor APIs"\n',
    );
    const h = buildHarness({ agentRoster: rosterDir });
    const bad = await createGroup(h, [
      { slot: "s", label: "A", title: "t", agentId: "not-in-roster" },
    ], { path: "roster" });
    expect(bad.kind).toBe("error");
    expect(bad.data).toEqual({ agentId: "not-in-roster" });

    const good = await createGroup(h, [{ slot: "s", label: "A", title: "t", agentId: "api-refactor" }], {
      path: "roster",
    });
    expect(good.kind).toBe("result");
    expect(good.result.group.members[0]).toMatchObject({ source: "roster", agentId: "api-refactor" });
  });
});

describe("shannon/group.message + the deterministic chain (§S B6.0-2/3)", () => {
  it("message → member A turn streams on the group thread → handoff → member B → group-completed", async () => {
    const h = buildHarness({
      turnScripts: [[textEvent("A 的产出")], [textEvent("M 的产出")]],
    });
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId;

    const res: any = await h.handlers["shannon/group.message"]!(
      { groupId, text: "出发" },
      h.initiator,
    );
    expect(res.kind).toBe("result");
    expect(res.result.messageId).toMatch(/^msg-/);
    expect(typeof res.result.ts).toBe("string");

    await vi.waitFor(() => {
      const events = eventsOf(h.bystander);
      expect(events.some((e) => e.type === "group.system" && e.system.kind === "group-completed")).toBe(true);
    });

    // Broadcast: BOTH devices saw the same group stream (host-level entity).
    const events = eventsOf(h.bystander);
    expect(eventsOf(h.initiator).map((e) => e.type)).toEqual(events.map((e) => e.type));
    const types = events.map((e) => e.type);
    // A streams → A terminal → handoff → member status flips → B streams →
    // B terminal → group-completed card.
    expect(types).toEqual([
      "group.member", // A working
      "task.progress", // A content
      "group.member", // A done
      "task.message", // A terminal
      "group.handoff",
      "group.system", // handoff card
      "group.member", // B working
      "task.progress", // B content
      "group.member", // B done
      "task.message", // B terminal
      "group.system", // group-completed
    ]);
    // Every stream frame carries the GROUP thread key, not a task id.
    for (const e of events) {
      expect(e.session_id).toBe(groupId);
    }
    expect(events.find((e) => e.type === "group.handoff").handoff).toMatchObject({
      from: "mem-01",
      to: "mem-02",
    });

    // Transcript keeps the honest replay: user → member entries → system.
    const history = groupHistoryLookup(h.groupsDirs)(groupId, {});
    expect(history).not.toBeNull();
    const contents = history!.messages.map((m) => m.content);
    expect(contents[0]).toContain("群已建立"); // the seed system card
    expect(contents).toContain("出发");
    expect(contents).toContain("A 的产出");
    expect(contents).toContain("M 的产出");
    expect(contents.some((c) => c.includes("已交接"))).toBe(true);
    // §S5 attribution keys ride the §J2 entries (mobile renders the member
    // row from them); the plain user entry stays attribution-free.
    const memberEntry = history!.messages.find((m) => m.kind === "member");
    expect(memberEntry).toBeDefined();
    expect(memberEntry!.member).toMatchObject({ memberId: "mem-01", label: "A" });
    expect(history!.messages[0]!.kind).toBe("system");
    expect(history!.messages.find((m) => m.role === "user")!.kind).toBe("user");
  });

  it("member failure stops the chain (no phantom handoff), failed status is honest", async () => {
    const h = buildHarness({
      turnScripts: [[{ type: "failed", error: "engine exploded" } as EngineEvent]],
    });
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId;
    await h.handlers["shannon/group.message"]!({ groupId, text: "go" }, h.initiator);
    await vi.waitFor(() => {
      expect(eventsOf(h.initiator).some((e) => e.type === "group.system" && e.system.kind === "member-failed")).toBe(true);
    });
    const types = eventsOf(h.initiator).map((e) => e.type);
    expect(types).not.toContain("group.handoff");
    // A failed turn has NO terminal reply — the honest card is the only close.
    expect(types).not.toContain("task.message");
    // Second member never ran (one client per turn; only A's was built).
    expect(h.clients).toHaveLength(1);
  });

  it("mention routes to the named member; unknown mention / archived group are INVALID_PARAMS", async () => {
    const h = buildHarness({ turnScripts: [[textEvent("S 的清单")]] });
    const created = await createGroup(h, [
      { slot: "flights", label: "A", title: "订机票" },
      { slot: "visa", label: "S", title: "查签证材料" },
    ]);
    const groupId = created.result.group.groupId;
    const bad: any = await h.handlers["shannon/group.message"]!(
      { groupId, text: "x", mentionMemberId: "mem-99" },
      h.initiator,
    );
    expect(bad.kind).toBe("error");
    const ok: any = await h.handlers["shannon/group.message"]!(
      { groupId, text: "@S 今天要", mentionMemberId: "mem-02" },
      h.initiator,
    );
    expect(ok.kind).toBe("result");
    await vi.waitFor(() => expect(h.clients.length).toBeGreaterThanOrEqual(1));
    // The mentioned member (mem-02) ran, not the plan head.
    const prompt = (h.clients[0]!.runQuery as any).mock.calls[0][0] as string;
    expect(prompt).toContain("查签证材料");
    expect(prompt).toContain("@S 今天要");

    const unknown: any = await h.handlers["shannon/group.message"]!(
      { groupId: "grp-00000000-0000-0000-0000-000000000000", text: "x" },
      h.initiator,
    );
    expect(unknown.kind).toBe("error");

    const arch: any = await h.handlers["shannon/group.archive"]!(
      { groupId, reason: "disbanded" },
      h.initiator,
    );
    expect(arch.result).toEqual({ ok: true });
    const after: any = await h.handlers["shannon/group.message"]!({ groupId, text: "x" }, h.initiator);
    expect(after.kind).toBe("error");
    expect(after.message).toContain("archived");
  });

  it("approval round-trip: §S group attribution on the ask, hub settle resumes the turn", async () => {
    const posts: Array<{ url: string; body: unknown }> = [];
    const fetchImpl = (async (url: any, init?: any) => {
      posts.push({ url: String(url), body: JSON.parse(init.body) });
      return new Response("{}", { status: 200 });
    }) as typeof fetch;
    const h = buildHarness({
      turnScripts: [
        [
          {
            type: "approval_request",
            request_id: "req-1",
            tool_name: "Bash",
            tool_input: { command: "book" },
            description: "book it",
            is_destructive: false,
            diff_preview: null,
          } as EngineEvent,
          textEvent("done after approval"),
        ],
      ],
      fetchImpl,
    });
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId;
    const res = await h.handlers["shannon/group.message"]!({ groupId, text: "go" }, h.initiator);
    expect(res.kind).toBe("result");

    await vi.waitFor(() => expect(h.hub.hasPendingApproval("dev-1")).toBe(true));
    const ask = eventsOf(h.initiator).find((e) => e.type === "approval.request");
    expect(ask.group).toEqual({
      groupId,
      member: { memberId: "mem-01", label: "A", title: "订机票", source: "ephemeral" },
    });
    // No handoff happened → no handoff-first trigger (宁缺勿造).
    expect(ask.group.ruleTrigger).toBeUndefined();

    h.hub.settleApproval("req-1", "allow");
    await vi.waitFor(() => {
      const events = eventsOf(h.initiator);
      expect(events.some((e) => e.type === "task.message")).toBe(true);
    });
    expect(posts).toHaveLength(1);
    expect(posts[0]!.url).toBe("http://engine:33420/api/approval/respond");
    expect(posts[0]!.body).toMatchObject({ request_id: "req-1", choice: "allow_once" });
  });
});

describe("§J2 interception (groupHistoryLookup)", () => {
  it("answers grp-* keys from the transcript store with §J2 paging; other keys → null", async () => {
    const h = buildHarness({
      turnScripts: [[textEvent("甲")], [textEvent("乙")]],
    });
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "第一句" }, h.initiator);
    await h.handlers["shannon/group.message"]!({ groupId, text: "第二句" }, h.initiator);
    await vi.waitFor(() => {
      expect(groupHistoryLookup(h.groupsDirs)(groupId, {})!.messages.length).toBeGreaterThanOrEqual(5);
    });

    const lookup = groupHistoryLookup(h.groupsDirs);
    const full = lookup(groupId, {})!;
    expect(full.sessionId).toBe(groupId);
    expect(full.hasMore).toBe(false);
    // Seed system card first, then user/member entries in order.
    const users = full.messages.filter((m) => m.role === "user");
    expect(users.map((m) => m.content)).toEqual(["第一句", "第二句"]);
    expect(full.messages[0]!.role).toBe("assistant"); // the seed card

    const paged = lookup(groupId, { before: full.messages[3]!.ts!, limit: 2 })!;
    expect(paged.messages.map((m) => m.content)).toEqual([full.messages[1]!.content, full.messages[2]!.content]);
    expect(paged.hasMore).toBe(true);

    expect(lookup("sess-not-a-group", {})).toBeNull();
    expect(lookup("grp-00000000-0000-0000-0000-000000000000", {})).toBeNull();
  });
});

describe("group store persistence (§S ruling: host-side files)", () => {
  it("group.json + transcript.jsonl live under the injected root; archive keeps the transcript", async () => {
    const h = buildHarness({ turnScripts: [[textEvent("产出")]] });
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "开工" }, h.initiator);
    // The chain ends with the member's task.message terminal (B never runs:
    // after A is done the plan still owes B — the chain hands off and the
    // second client is built with an empty script → honest member-failed).
    await vi.waitFor(() => {
      expect(eventsOf(h.initiator).some((e) => e.type === "task.message")).toBe(true);
    });

    const dir = join(h.groupsDirs[0]!, groupId);
    expect(existsSync(join(dir, "group.json"))).toBe(true);
    expect(existsSync(join(dir, "transcript.jsonl"))).toBe(true);
    const transcript = readFileSync(join(dir, "transcript.jsonl"), "utf8").trim().split("\n");
    expect(transcript.length).toBeGreaterThanOrEqual(3); // system seed + user + member

    const arch: any = await h.handlers["shannon/group.archive"]!(
      { groupId, reason: "completed" },
      h.initiator,
    );
    expect(arch.result.ok).toBe(true);
    // A finished; B's empty script failed — the aggregate is as-lived (1/2).
    expect(arch.result.review).toMatchObject({ goals: { done: 1, total: 2 }, decisionCount: 0 });
    const list: any = await h.handlers["shannon/group.list"]!({}, h.initiator);
    expect(list.result.groups[0]).toMatchObject({ groupId, status: "completed" });
    // Transcript survives the archive (回看口径).
    expect(groupHistoryLookup(h.groupsDirs)(groupId, {})).not.toBeNull();
  });
});

// ── §S B6.1: the pool ledger chain (amount 口径 → escalation → settlement) ──

function approvalEvent(
  requestId: string,
  toolInput: unknown,
  over: { toolName?: string; description?: string } = {},
): EngineEvent {
  return {
    type: "approval_request",
    request_id: requestId,
    tool_name: over.toolName ?? "payments.transfer",
    tool_input: toolInput,
    description: over.description ?? "支付一笔款项",
    is_destructive: false,
    diff_preview: null,
  } as EngineEvent;
}

describe("§S B6.1 pool ledger (member-turn approvals with amounts)", () => {
  it("allow: pending → ledger line + group.pool-spend broadcast + pool/member numbers agree (decidedBy = the deciding device)", async () => {
    const posts: Array<{ url: string; body: unknown }> = [];
    const h = buildHarness({
      // B6.2 note: an explicit crew keeps the planning query out of the way —
      // omitted members would run it and consume this test's first script.
      turnScripts: [[approvalEvent("req-pay-1", { amountCny: 120.5 }), textEvent("付好了")]],
      fetchImpl: (async (url: any, init?: any) => {
        posts.push({ url: String(url), body: JSON.parse(init.body) });
        return new Response("{}", { status: 200 });
      }) as typeof fetch,
    });
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "订机票" }, h.initiator);
    await vi.waitFor(() => expect(h.hub.hasPendingApproval("dev-1")).toBe(true));

    // At ask time: the amount sits in pending, and the ask carries poolAfter
    // (remainingAfter = total − spent − pending, honest arithmetic).
    const ask = eventsOf(h.initiator).find((e) => e.type === "approval.request")!;
    expect(ask.group.poolAfter).toEqual({ poolCny: 8000, remainingAfterCny: 7879.5 });
    let list: any = await h.handlers["shannon/group.list"]!({}, h.initiator);
    expect(list.result.groups[0].pool).toMatchObject({ spentCny: 0, pendingCny: 120.5 });

    h.hub.settleApproval("req-pay-1", "allow");
    await vi.waitFor(() => {
      expect(eventsOf(h.bystander).some((e) => e.type === "group.pool-spend")).toBe(true);
    });
    // The wire mirror of the ledger line, on BOTH devices (host-level entity).
    const spend = eventsOf(h.bystander).find((e) => e.type === "group.pool-spend")!.poolSpend;
    expect(spend).toMatchObject({
      memberId: "mem-01",
      amountCny: 120.5,
      kind: "payments.transfer",
      approvalId: "req-pay-1",
      decidedBy: "dev-1",
      poolAfterCny: 120.5,
    });
    expect(spend.id).toMatch(/^ps-/);

    // The ledger file took exactly one line; the group.list projection's
    // spent equals the ledger's running sum (the two-books invariant).
    const ledger = readPoolLedger(h.groupsDirs, groupId);
    expect(ledger).toHaveLength(1);
    expect(ledger[0]).toMatchObject({ amountCny: 120.5, approvalId: "req-pay-1", decidedBy: "dev-1" });
    list = await h.handlers["shannon/group.list"]!({}, h.initiator);
    expect(list.result.groups[0].pool).toMatchObject({ spentCny: 120.5, pendingCny: 0 });
    expect(list.result.groups[0].pool.spentCny).toBe(
      Math.round(ledger.reduce((s, e) => s + e.amountCny, 0) * 100) / 100,
    );
    // The member's own spentCny moved with the pool (persisted on the entity).
    const stored = loadGroup(h.groupsDirs, groupId)!;
    expect(stored.members[0]).toMatchObject({ spentCny: 120.5 });
    expect(stored.pool.pendingCny).toBe(0);
    // The engine got the allow.
    expect(posts[0]!.body).toMatchObject({ request_id: "req-pay-1", choice: "allow_once" });
  });

  it("deny: the pending refunds and the ledger takes nothing (拒绝不记账)", async () => {
    const h = buildHarness({
      turnScripts: [[approvalEvent("req-deny-1", { amountCny: 88 }), textEvent("换方案继续")]],
    });
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "试试" }, h.initiator);
    await vi.waitFor(() => expect(h.hub.hasPendingApproval("dev-1")).toBe(true));
    h.hub.settleApproval("req-deny-1", "deny");
    await vi.waitFor(() => {
      expect(eventsOf(h.initiator).some((e) => e.type === "task.message")).toBe(true);
    });
    expect(readPoolLedger(h.groupsDirs, groupId)).toEqual([]);
    expect(eventsOf(h.initiator).some((e) => e.type === "group.pool-spend")).toBe(false);
    const list: any = await h.handlers["shannon/group.list"]!({}, h.initiator);
    expect(list.result.groups[0].pool).toMatchObject({ spentCny: 0, pendingCny: 0 });
  });

  it("no parseable amount → the entire pool path is bypassed (no poolAfter, no pending, no ledger)", async () => {
    const h = buildHarness({
      turnScripts: [[approvalEvent("req-plain-1", { command: "ls -la" }), textEvent("做完了")]],
    });
    // Same slot/label/title as the generic template's head, spelled out so the
    // B6.2 planning query stays out of this pool-path test.
    const created = await createGroup(h, [{ slot: "plan", label: "A", title: "规划分工" }]);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "看看" }, h.initiator);
    await vi.waitFor(() => expect(h.hub.hasPendingApproval("dev-1")).toBe(true));
    const ask = eventsOf(h.initiator).find((e) => e.type === "approval.request")!;
    expect(ask.group).toEqual({
      groupId,
      member: { memberId: "mem-01", label: "A", title: "规划分工", source: "ephemeral" },
    });
    h.hub.settleApproval("req-plain-1", "allow");
    await vi.waitFor(() => {
      expect(eventsOf(h.initiator).some((e) => e.type === "task.message")).toBe(true);
    });
    expect(readPoolLedger(h.groupsDirs, groupId)).toEqual([]);
    expect(eventsOf(h.initiator).some((e) => e.type === "group.pool-spend")).toBe(false);
    const list: any = await h.handlers["shannon/group.list"]!({}, h.initiator);
    expect(list.result.groups[0].pool).toMatchObject({ spentCny: 0, pendingCny: 0 });
  });

  it("value (分) converts ÷100 into the ledger; over-pool / over-share escalate with honest negative remainingAfter", async () => {
    // over-pool: spent(0) + pending(0) + 9000 > 8000 → the ask is labeled,
    // allowed anyway (approvals are the user's call), and remainingAfter
    // ships NEGATIVE (-1000) as-is.
    const posts: Array<{ url: string; body: unknown }> = [];
    const h = buildHarness({
      turnScripts: [[approvalEvent("req-over-1", { value: 900000 }), textEvent("超了但用户同意")]],
      fetchImpl: (async (url: any, init?: any) => {
        posts.push({ url: String(url), body: JSON.parse(init.body) });
        return new Response("{}", { status: 200 });
      }) as typeof fetch,
    });
    const created = await createGroup(h, [
      { slot: "flights", label: "A", title: "订机票", shareCny: 100 },
    ]);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "订" }, h.initiator);
    await vi.waitFor(() => expect(h.hub.hasPendingApproval("dev-1")).toBe(true));
    const ask = eventsOf(h.initiator).find((e) => e.type === "approval.request")!;
    expect(ask.group.ruleTrigger).toBe("over-pool"); // pool escalation before the share one
    expect(ask.group.poolAfter).toEqual({ poolCny: 8000, remainingAfterCny: -1000 });
    h.hub.settleApproval("req-over-1", "allow");
    await vi.waitFor(() => {
      expect(eventsOf(h.initiator).some((e) => e.type === "group.pool-spend")).toBe(true);
    });
    // value 900000 分 = 9000 元 in the ledger; member.spent (9000) > share (100).
    const ledger = readPoolLedger(h.groupsDirs, groupId);
    expect(ledger[0]).toMatchObject({ amountCny: 9000, poolAfterCny: 9000 });
    const list: any = await h.handlers["shannon/group.list"]!({}, h.initiator);
    expect(list.result.groups[0].pool).toMatchObject({ spentCny: 9000, pendingCny: 0 });
  });

  it("over-share fires when the ask passes the member's remaining share but stays inside the pool", async () => {
    const h = buildHarness({
      turnScripts: [[approvalEvent("req-share-1", { amountCny: 150 }), textEvent("好")]],
    });
    const created = await createGroup(h, [
      { slot: "flights", label: "A", title: "订机票", shareCny: 100 },
    ]);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "订" }, h.initiator);
    await vi.waitFor(() => expect(h.hub.hasPendingApproval("dev-1")).toBe(true));
    const ask = eventsOf(h.initiator).find((e) => e.type === "approval.request")!;
    expect(ask.group.ruleTrigger).toBe("over-share"); // 150 > 100 − 0, pool unbroken
    expect(ask.group.poolAfter).toEqual({ poolCny: 8000, remainingAfterCny: 7850 });
  });

  it("handoff-first keeps the single trigger key; the over-* verdict is still enforced underneath", async () => {
    const h = buildHarness({
      turnScripts: [
        [textEvent("A 的产出")],
        [approvalEvent("req-hf-1", { amountCny: 9000 }), textEvent("B 完成")],
      ],
    });
    // Explicit two-member crew: the ask under test is the POST-handoff one
    // (script #1), so the B6.2 planning query must not consume it.
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "开工" }, h.initiator);
    await vi.waitFor(() => expect(h.hub.hasPendingApproval("dev-1")).toBe(true));
    // B's ask (after the A→B handoff) would ALSO be over-pool — but only one
    // key rides, and the red line wins the label.
    const ask = eventsOf(h.initiator).filter((e) => e.type === "approval.request")[0]!;
    expect(ask.group.ruleTrigger).toBe("handoff-first");
    // The pool mechanics are NOT skipped — poolAfter still rides the ask.
    expect(ask.group.poolAfter).toEqual({ poolCny: 8000, remainingAfterCny: -1000 });
    h.hub.settleApproval("req-hf-1", "allow");
    await vi.waitFor(() => {
      expect(eventsOf(h.initiator).some((e) => e.type === "group.pool-spend")).toBe(true);
    });
    expect(readPoolLedger(h.groupsDirs, groupId)).toHaveLength(1);
  });

  it("quote-window expiry: the ask outlives the hub default, dies at ITS window, cards + refunds + denies", async () => {
    const posts: Array<{ url: string; body: unknown }> = [];
    const h = buildHarness({
      turnScripts: [[approvalEvent("req-exp-1", { amountCny: 66 }), textEvent("过期后继续")]],
      // The amount ask carries its own quote window (200ms here): it must
      // OUTLIVE the hub-wide default (40ms — TTL 生效) and die at ITS window.
      approvalTimeoutMs: 40,
      quoteWindowMs: 200,
      fetchImpl: (async (url: any, init?: any) => {
        posts.push({ url: String(url), body: JSON.parse(init.body) });
        return new Response("{}", { status: 200 });
      }) as typeof fetch,
    });
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "订" }, h.initiator);
    await vi.waitFor(() => expect(h.hub.hasPendingApproval("dev-1")).toBe(true));
    // Well past the hub-wide default (40ms) the ask is STILL parked — the
    // per-ask quote window overrode the default (plain asks would be gone).
    await new Promise((r) => setTimeout(r, 140));
    expect(h.hub.hasPendingApproval("dev-1")).toBe(true);
    // …and at the quote window the honest expiry path fires.
    await vi.waitFor(() => {
      expect(
        eventsOf(h.initiator).some((e) => e.type === "group.system" && e.system.kind === "quote-expired"),
      ).toBe(true);
    });
    const card = eventsOf(h.initiator).find((e) => e.system?.kind === "quote-expired")!;
    expect(card.session_id).toBe(groupId);
    expect(card.system.text).toContain("窗口内未决定");
    expect(card.system.text).toContain("已自动放弃");
    expect(typeof card.system.ts).toBe("string");
    // The turn continued after the expiry-deny (engine forwards it).
    await vi.waitFor(() => {
      expect(eventsOf(h.initiator).some((e) => e.type === "task.message")).toBe(true);
    });
    // Pending refunded, ledger untouched, and the engine was told "deny".
    const list: any = await h.handlers["shannon/group.list"]!({}, h.initiator);
    expect(list.result.groups[0].pool).toMatchObject({ spentCny: 0, pendingCny: 0 });
    expect(readPoolLedger(h.groupsDirs, groupId)).toEqual([]);
    expect(posts[0]!.body).toMatchObject({ request_id: "req-exp-1", choice: "deny" });
    // The card is in the transcript for replay.
    const history = groupHistoryLookup(h.groupsDirs)(groupId, {});
    expect(history!.messages.some((m) => m.content.includes("已自动放弃"))).toBe(true);
  });

  it("an amount-bearing ask rides the R4 quote window on the wire and the engine TTL; expiresAt mirrors the parking timer", async () => {
    const fixedNow = 1_770_000_000_000;
    const h = buildHarness({
      turnScripts: [[approvalEvent("req-qw-1", { amountCny: 3480 }), textEvent("窗口内成交")]],
      now: () => fixedNow, // fixed clock → expiresAt is exact, not a moving tick
    });
    const created = await createGroup(h, [
      { slot: "flights", label: "A", title: "订机票", shareCny: 3600 },
    ]);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "订" }, h.initiator);
    await vi.waitFor(() => expect(h.hub.hasPendingApproval("dev-1")).toBe(true));
    const ask = eventsOf(h.initiator).find((e) => e.type === "approval.request")!;
    // The R4 countdown contract (12 「14:41 前有效」): 30 minutes, constant
    // onExpire, expiresAt == the instant the hub will auto-abandon the ask.
    expect(ask.group.quoteWindow).toEqual({
      expiresAt: new Date(fixedNow + 30 * 60 * 1000).toISOString(),
      windowMinutes: 30,
      onExpire: "requote-next",
    });
    // The engine side of the same window: the member-turn query frame rides
    // `approval_ttl_ms` = the quote window (engine hard cap: 60 minutes).
    const queryOpts = (h.clients[0]!.runQuery as any).mock.calls[0]?.[1] ?? {};
    expect(queryOpts.approvalTtlMs).toBe(1_800_000);
    // The registry entry's retention mirrors the same window (not the 330s
    // default) — approval.list / snapshot can render the countdown.
    const pending = h.approvals.listPending();
    expect(pending[0]!.group?.quoteWindow).toEqual(ask.group.quoteWindow);
  });

  it("the quote-window override never leaves the group face: plain queries carry no approvalTtlMs", async () => {
    // The create-time crew PLANNING query is host-internal and non-amount:
    // it must not run under the quote window (open question #3 收尾 path).
    const h = buildHarness({
      turnScripts: [[textEvent("推荐阵容")]],
    });
    await createGroup(h, undefined); // ephemeral, no members → planning query
    await vi.waitFor(() => expect(h.clients.length).toBeGreaterThan(0));
    const planningOpts = (h.clients[0]!.runQuery as any).mock.calls[0]?.[1] ?? {};
    expect(planningOpts).not.toHaveProperty("approvalTtlMs");
  });
});

describe("shannon/group.get + the review aggregate (§S B6.2)", () => {
  it("serves the entity; unknown / malformed groupId are INVALID_PARAMS; the pairing gate holds", async () => {
    const h = buildHarness();
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId as string;

    const got: any = await h.handlers["shannon/group.get"]!({ groupId }, h.initiator);
    expect(got.kind).toBe("result");
    expect(got.result.group).toMatchObject({ groupId, status: "active" });
    // An active group honestly has no review yet.
    expect(got.result.group.review).toBeUndefined();

    const unknown: any = await h.handlers["shannon/group.get"]!(
      { groupId: "grp-00000000-0000-0000-0000-000000000000" },
      h.initiator,
    );
    expect(unknown.kind).toBe("error");
    expect(unknown.code).toBe(-32001);

    const malformed: any = await h.handlers["shannon/group.get"]!({ groupId: "not-a-group" }, h.initiator);
    expect(malformed.kind).toBe("error");
    expect(malformed.code).toBe(-32001);

    const unpaired: any = await h.handlers["shannon/group.get"]!({ groupId }, fakeCtx(null));
    expect(unpaired.kind).toBe("error");
    expect(unpaired.code).toBe(-32000);
  });

  it("archive(completed) aggregates the review from as-lived state, persists it, and get serves it", async () => {
    const h = buildHarness({
      turnScripts: [[textEvent("A 的产出")], [textEvent("M 的产出")]],
    });
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId as string;
    await h.handlers["shannon/group.message"]!({ groupId, text: "开工" }, h.initiator);
    await vi.waitFor(() => {
      expect(readTranscript(h.groupsDirs, groupId).some((e) => e.systemKind === "handoff")).toBe(true);
    });

    // The pool rows come from group.json alone (B6.1's ledger will feed the
    // same fields later) — simulate one member having spent, honestly.
    const record = loadGroup(h.groupsDirs, groupId)!;
    record.members[0]!.spentCny = 123.456;
    record.pool.spentCny = 123.456;
    saveGroup(h.groupsDirs, record);

    const arch: any = await h.handlers["shannon/group.archive"]!(
      { groupId, reason: "completed" },
      h.initiator,
    );
    expect(arch.kind).toBe("result");
    const review = arch.result.review;
    expect(review).toBeDefined();
    // 2/2 done (both members finished before the flip), one marked handoff
    // card, decisionCount an honest 0 (no approval-attribution face in v1).
    expect(review.goals).toEqual({ done: 2, total: 2 });
    expect(review.handoffCount).toBe(1);
    expect(review.decisionCount).toBe(0);
    expect(typeof review.durationMinutes).toBe("number");
    expect(review.pool).toEqual({
      totalCny: 8000,
      spentCny: 123.456,
      perMember: [
        { memberId: "mem-01", label: "A", title: "订机票", amountCny: 123.46 },
      ],
    });
    expect(review.deliverables).toEqual([
      {
        memberId: "mem-01",
        title: "订机票",
        status: "已完成",
        lines: [{ k: "产出摘要", v: "A 的产出" }],
        artifact: null,
      },
      {
        memberId: "mem-02",
        title: "订酒店",
        status: "已完成",
        lines: [{ k: "产出摘要", v: "M 的产出" }],
        artifact: null,
      },
    ]);

    // Persisted into group.json and served by group.get.
    expect(loadGroup(h.groupsDirs, groupId)!.review).toEqual(review);
    const got: any = await h.handlers["shannon/group.get"]!({ groupId }, h.initiator);
    expect(got.result.group.review).toEqual(review);
    // group.list never carries it.
    const list: any = await h.handlers["shannon/group.list"]!({}, h.initiator);
    expect(list.result.groups[0].review).toBeUndefined();
  });

  it("archive(disbanded) honestly omits the review — response and store both", async () => {
    const h = buildHarness({ turnScripts: [[textEvent("产出")]] });
    const created = await createGroup(h, TWO_MEMBER_PLAN);
    const groupId = created.result.group.groupId as string;
    const arch: any = await h.handlers["shannon/group.archive"]!(
      { groupId, reason: "disbanded" },
      h.initiator,
    );
    expect(arch.result).toEqual({ ok: true });
    expect(loadGroup(h.groupsDirs, groupId)!.review).toBeUndefined();
    const got: any = await h.handlers["shannon/group.get"]!({ groupId }, h.initiator);
    expect(got.result.group.review).toBeUndefined();
  });
});

describe("the daily report tick (§S B6.2, R11)", () => {
  /** Local wall-clock timestamp at HH:mm, `dayOffset` days from today — the
   *  report contract is LOCAL timezone. */
  function localAt(hours: number, minutes: number, dayOffset = 0): number {
    const d = new Date();
    d.setDate(d.getDate() + dayOffset);
    d.setHours(hours, minutes, 0, 0);
    return d.getTime();
  }

  function reportsOf(ctx: MethodContext): any[] {
    return eventsOf(ctx).filter((e) => e.type === "group.report");
  }

  it("fires at the configured local HH:mm, enters the transcript, dedupes per day, and stands down after archive", async () => {
    let current = localAt(10, 0);
    const scheduled: Array<() => void> = [];
    const h = buildHarness({
      now: () => current,
      reportScheduleNext: (fn) => {
        scheduled.push(fn);
        return () => {};
      },
    });
    const created = await createGroup(h, TWO_MEMBER_PLAN, {
      rules: { dailyReportAt: "23:00" },
    });
    const groupId = created.result.group.groupId as string;
    // The create armed the tick (one eligible group now exists).
    expect(scheduled).toHaveLength(1);

    // A tick away from the report minute: nothing.
    current = localAt(22, 59);
    scheduled.shift()!();
    expect(reportsOf(h.initiator)).toHaveLength(0);

    // 23:00 → one report on the group thread, broadcast to BOTH devices.
    current = localAt(23, 0);
    scheduled.shift()!();
    const reports = reportsOf(h.initiator);
    expect(reports).toHaveLength(1);
    expect(reports[0].session_id).toBe(groupId);
    expect(reports[0].report).toMatchObject({
      date: localDateKey(new Date(current)),
      kind: "daily",
      goalProgress: { done: 0, total: 2 },
      todaySpentCny: 0,
    });
    // Honest text: the 0 spend is declared a lower bound, never silent.
    expect(reports[0].report.text).toContain("目标进度：0/2");
    expect(reports[0].report.text).toContain("如实下界");
    expect(reportsOf(h.bystander)).toHaveLength(1);
    // The thread keeps a replay copy.
    expect(readTranscript(h.groupsDirs, groupId).some((e) => e.content.includes("群日报"))).toBe(true);

    // The re-armed tick (same day, any minute) never duplicates the report.
    current = localAt(23, 1);
    scheduled.shift()!();
    expect(reportsOf(h.initiator)).toHaveLength(1);

    // Next day 23:00 → the day's report fires again.
    current = localAt(23, 0, 1);
    scheduled.shift()!();
    expect(reportsOf(h.initiator)).toHaveLength(2);

    // Archive the group → the chain stands down (no further reports, and the
    // tick found nothing eligible so it did NOT re-arm itself).
    await h.handlers["shannon/group.archive"]!({ groupId, reason: "completed" }, h.initiator);
    current = localAt(23, 0, 2);
    scheduled.shift()!();
    expect(reportsOf(h.initiator)).toHaveLength(2);
    expect(scheduled).toHaveLength(0);
  });

  it("stops with the host (bundle.stop clears the chain) and never arms without an eligible group", async () => {
    const scheduled: Array<() => void> = [];
    const h = buildHarness({
      reportScheduleNext: (fn) => {
        scheduled.push(fn);
        return () => {};
      },
    });
    // No group exists → construction armed nothing.
    expect(scheduled).toHaveLength(0);
    h.facet.stop();
    const created = await createGroup(h, TWO_MEMBER_PLAN, {
      rules: { dailyReportAt: "08:30" },
    });
    expect(created.kind).toBe("result");
    // A stopped timer stays stopped even when a group becomes eligible.
    expect(scheduled).toHaveLength(0);
  });
});

describe("the crew planning query (§S B6.2, 提案开放问题 3 收尾)", () => {
  it("plans the crew from one engine query; the prompt carries no phone-visible session key", async () => {
    const h = buildHarness({
      turnScripts: [
        [
          textEvent(
            '好的：[{"slot":"flights","label":"A","title":"订机票"},{"slot":"hotels","label":"M","title":"订酒店"}]',
          ),
        ],
      ],
    });
    const res = await createGroup(h, undefined);
    expect(res.kind).toBe("result");
    expect(res.result.group.members).toHaveLength(2);
    expect(res.result.group.members[0]).toMatchObject({
      slot: "flights",
      label: "A",
      title: "订机票",
      source: "ephemeral",
      agentId: null,
    });
    // Exactly one planning client was built and its prompt is the goal + the
    // JSON-array ask — never the device session id or a group key.
    expect(h.clients).toHaveLength(1);
    const prompt = (h.clients[0]!.runQuery as any).mock.calls[0][0] as string;
    expect(prompt).toContain("东京行程筹备");
    expect(prompt).toContain("推荐 2-4 人分工阵容");
    expect(prompt).not.toContain("dev-1");
    expect(prompt).not.toContain("grp-");
    expect(prompt).not.toContain("sess-");
  });

  it("falls back to the generic template on unusable output, engine failure, or timeout (诚实降级)", async () => {
    // Unparseable text → template.
    const garbage = buildHarness({ turnScripts: [[textEvent("我觉得三个人不错")]] });
    const g = await createGroup(garbage, undefined);
    expect(g.result.group.members).toHaveLength(3);
    expect(g.result.group.members.map((m: any) => m.slot)).toEqual(["plan", "do", "verify"]);

    // Engine-side failure frame → template.
    const failed = buildHarness({
      turnScripts: [[{ type: "failed", error: "boom" } as EngineEvent]],
    });
    const f = await createGroup(failed, undefined);
    expect(f.result.group.members).toHaveLength(3);

    // A hung engine burns the budget, then degrades — create never wedges.
    const hungDirs = [mkdtempSync(join(tmpdir(), "shannon-groups-test-"))];
    const hungHub = new MobileDispatchHub({ logger });
    const hungFacet = createGroupHandlers({
      hub: hungHub,
      logger,
      now: () => 1_770_000_000_000,
      groupsDirs: hungDirs,
      engineWsUrl: "ws://engine:33420/api/ws",
      engineHttpBaseUrl: "http://engine:33420",
      planningTimeoutMs: 25,
      engineClientFactory: () => hangingEngineClient(),
    });
    const hung: any = await hungFacet.handlers["shannon/group.create"]!(
      { goal: "x" },
      fakeCtx("dev-1"),
    );
    expect(hung.kind).toBe("result");
    expect(hung.result.group.members).toHaveLength(3);
    hungFacet.stop();
  });

  it("roster path never plans — omitted members still fail validation with the agentId error", async () => {
    const h = buildHarness();
    const res: any = await createGroup(h, undefined, { path: "roster" });
    expect(res.kind).toBe("error");
    expect(res.message).toContain("agentId");
    expect(h.clients).toHaveLength(0);
  });
});

afterEach(() => {
  vi.useRealTimers();
});
