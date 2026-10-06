import { describe, expect, it } from "vitest";

import {
  ApprovalRegistry,
  approvalWireItem,
  engineAgent,
  engineRisk,
} from "../approvalRegistry.js";
import type { PendingApprovalRecord } from "../approvalRegistry.js";

function rec(overrides: Partial<PendingApprovalRecord> = {}): PendingApprovalRecord {
  return {
    requestId: "req-1",
    toolName: "Write",
    toolInput: { path: "/tmp/a.txt" },
    description: "写文件",
    isDestructive: false,
    diffPreview: null,
    ts: Date.now(),
    ...overrides,
  };
}

describe("ApprovalRegistry", () => {
  it("records, lists pending (oldest first), and resolves by requestId", () => {
    const reg = new ApprovalRegistry();
    reg.record(rec({ requestId: "a" }));
    reg.record(rec({ requestId: "b" }));
    expect(reg.size).toBe(2);
    expect(reg.listPending().map((r) => r.requestId)).toEqual(["a", "b"]);
    expect(reg.resolve("a")).toBe(true);
    expect(reg.resolve("a")).toBe(false); // second resolve is a no-op
    expect(reg.listPending().map((r) => r.requestId)).toEqual(["b"]);
  });

  it("re-recording an id is a no-op (the first push is the ask)", () => {
    const reg = new ApprovalRegistry();
    reg.record(rec({ requestId: "a", description: "first" }));
    reg.record(rec({ requestId: "a", description: "second" }));
    expect(reg.size).toBe(1);
    expect(reg.listPending()[0]?.description).toBe("first");
  });

  it("sweeps entries older than maxAgeMs on read (the engine denies at 300s)", () => {
    let t = 5_000;
    const reg = new ApprovalRegistry({ now: () => t, maxAgeMs: 330_000 });
    reg.record(rec({ requestId: "fresh", ts: t }));
    reg.record(rec({ requestId: "stale", ts: t - 331_000 }));
    expect(reg.listPending().map((r) => r.requestId)).toEqual(["fresh"]);
    t += 330_001;
    expect(reg.listPending()).toEqual([]);
  });

  it("caps the ring by dropping the oldest entry", () => {
    const reg = new ApprovalRegistry({ maxEntries: 2 });
    reg.record(rec({ requestId: "a" }));
    reg.record(rec({ requestId: "b" }));
    reg.record(rec({ requestId: "c" }));
    expect(reg.listPending().map((r) => r.requestId)).toEqual(["b", "c"]);
  });
});

describe("approvalWireItem (the approvalFromMap contract)", () => {
  it("maps the required keys: approvalId/kind/headline/risk/timestamp/toolInput", () => {
    const item = approvalWireItem(
      rec({ isDestructive: true, ts: Date.UTC(2026, 9, 3, 1, 2, 3) }),
    );
    expect(item).toEqual({
      approvalId: "req-1",
      kind: "Write",
      headline: "写文件",
      risk: "high",
      timestamp: "2026-10-03T01:02:03.000Z",
      toolInput: { path: "/tmp/a.txt" },
      diffTitle: "/tmp/a.txt",
    });
  });

  it("synthesizes the risk band from the engine's three-dimensional risk", () => {
    const base = { isDestructive: false };
    expect(approvalWireItem(rec({ ...base, risk: { scope: "system", reversible: true } })).risk).toBe("high");
    expect(approvalWireItem(rec({ ...base, risk: { scope: "repo", reversible: false } })).risk).toBe("medium");
    expect(approvalWireItem(rec({ ...base, risk: { scope: "repo", reversible: true } })).risk).toBe("low");
    expect(approvalWireItem(rec({ ...base, risk: { destructive: true, scope: "repo", reversible: true } })).risk).toBe("high");
    // Without the engine risk, the legacy is_destructive bit decides.
    expect(approvalWireItem(rec({ isDestructive: true })).risk).toBe("high");
    expect(approvalWireItem(rec()).risk).toBe("low");
  });

  it("omits optional keys the engine did not supply (never invents)", () => {
    const item = approvalWireItem(rec({ toolInput: { command: "ls" } }));
    expect(item).not.toHaveProperty("diffTitle");
    expect(item).not.toHaveProperty("agentId");
    expect(item).not.toHaveProperty("agentName");
    expect(item).not.toHaveProperty("scope");
  });

  it("carries the engine agent attribution when present", () => {
    const item = approvalWireItem(rec({ agent: { id: "agent-1", name: "Builder" } }));
    expect(item.agentId).toBe("agent-1");
    expect(item.agentName).toBe("Builder");
  });
});

describe("engine rich-field normalizers", () => {
  it("engineAgent tolerates nulls and passthrough junk", () => {
    expect(engineAgent(null)).toBeNull();
    expect(engineAgent(undefined)).toBeNull();
    expect(engineAgent({})).toBeNull();
    expect(engineAgent({ id: "a", name: null })).toEqual({ id: "a", name: "" });
    expect(engineAgent({ id: null, name: "Builder" })).toEqual({ id: "", name: "Builder" });
  });

  it("engineRisk requires scope+reversible and keeps an optional destructive", () => {
    expect(engineRisk(null)).toBeNull();
    expect(engineRisk({ scope: "nope", reversible: true })).toBeNull();
    expect(engineRisk({ scope: "repo" })).toBeNull();
    expect(engineRisk({ scope: "local", reversible: false })).toEqual({
      scope: "local",
      reversible: false,
    });
    expect(engineRisk({ destructive: true, scope: "repo", reversible: true })).toEqual({
      destructive: true,
      scope: "repo",
      reversible: true,
    });
  });
});
