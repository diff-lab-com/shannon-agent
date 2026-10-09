/**
 * §S B6.1 pool ledger store — the honest-money mechanics behind the group
 * orchestrator: the `tool_input` amount 口径 (amountCny 元 first, value 分
 * ÷100, nothing invented), the append-only `ledger.jsonl` with the
 * transcript's torn-tail convention, and the id/rounding helpers.
 * The full orchestration chain (pending → settle → broadcast) is pinned in
 * `groupHandlers.test.ts`; this file pins the STORE contract.
 */

import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  appendPoolSpend,
  newGroupId,
  newPoolSpendId,
  readPoolLedger,
  resolvePoolSpendAmountCny,
  round2Cny,
  type PoolSpendEntry,
} from "../groupStore.js";

function tmpDirs(): string[] {
  return [mkdtempSync(join(tmpdir(), "shannon-ledger-test-"))];
}

function entry(overrides: Partial<PoolSpendEntry> = {}): PoolSpendEntry {
  return {
    id: newPoolSpendId(),
    memberId: "mem-01",
    amountCny: 12.34,
    kind: "Bash",
    approvalId: "req-1",
    decidedBy: "dev-1",
    poolAfterCny: 12.34,
    ts: "2026-10-09T00:00:00.000Z",
    ...overrides,
  };
}

describe("resolvePoolSpendAmountCny (the B6.1 honest 口径)", () => {
  it("amountCny (元) wins outright", () => {
    expect(resolvePoolSpendAmountCny({ amountCny: 19.9, value: 1 })).toBe(19.9);
    expect(resolvePoolSpendAmountCny({ amountCny: 0 })).toBe(0);
  });

  it("falls back to value (分, 引擎载荷原文口径) ÷ 100", () => {
    expect(resolvePoolSpendAmountCny({ value: 1990 })).toBe(19.9);
    expect(resolvePoolSpendAmountCny({ value: 1 })).toBe(0.01);
    expect(resolvePoolSpendAmountCny({ value: 0 })).toBe(0);
  });

  it("both absent / non-object input → null (no pool accounting, never an estimate)", () => {
    expect(resolvePoolSpendAmountCny({ command: "ls" })).toBeNull();
    expect(resolvePoolSpendAmountCny({})).toBeNull();
    expect(resolvePoolSpendAmountCny(null)).toBeNull();
    expect(resolvePoolSpendAmountCny("1990")).toBeNull();
    expect(resolvePoolSpendAmountCny(undefined)).toBeNull();
  });

  it("non-finite / negative / wrong-typed fields count as ABSENT (fall through, not 0)", () => {
    // amountCny unusable → the value path still gets its chance.
    expect(resolvePoolSpendAmountCny({ amountCny: -5, value: 500 })).toBe(5);
    expect(resolvePoolSpendAmountCny({ amountCny: "19.9", value: 500 })).toBe(5);
    expect(resolvePoolSpendAmountCny({ amountCny: Number.NaN, value: 500 })).toBe(5);
    expect(resolvePoolSpendAmountCny({ amountCny: Number.POSITIVE_INFINITY, value: 500 })).toBe(5);
    // Both unusable → null.
    expect(resolvePoolSpendAmountCny({ amountCny: -5, value: -1 })).toBeNull();
    expect(resolvePoolSpendAmountCny({ amountCny: true, value: "500" })).toBeNull();
    expect(resolvePoolSpendAmountCny({ value: Number.NaN })).toBeNull();
    // A usable 0 is real money (free) — not "absent".
    expect(resolvePoolSpendAmountCny({ amountCny: "x", value: 0 })).toBe(0);
  });
});

describe("round2Cny", () => {
  it("keeps 元 at two decimals without float drift", () => {
    expect(round2Cny(0.1 + 0.2)).toBe(0.3);
    expect(round2Cny(19.999)).toBe(20);
    expect(round2Cny(1.005)).toBe(1.0); // banker's-free: Math.round semantics
  });
});

describe("ledger.jsonl (append-only, host-internal)", () => {
  it("appends and reads back oldest-first; ids are ps- prefixed and unique", () => {
    const dirs = tmpDirs();
    const groupId = newGroupId();
    appendPoolSpend(dirs, groupId, entry({ amountCny: 1, poolAfterCny: 1 }));
    appendPoolSpend(dirs, groupId, entry({ amountCny: 2, poolAfterCny: 3 }));
    const ledger = readPoolLedger(dirs, groupId);
    expect(ledger.map((e) => e.amountCny)).toEqual([1, 2]);
    expect(ledger[0]!.id).toMatch(/^ps-/);
    expect(ledger[0]!.id).not.toBe(ledger[1]!.id);
    // One JSON object per line, newline-terminated (the jsonl convention).
    const raw = readFileSync(join(dirs[0]!, groupId, "ledger.jsonl"), "utf8");
    expect(raw.endsWith("\n")).toBe(true);
    expect(raw.trim().split("\n")).toHaveLength(2);
  });

  it("skips a torn tail line (crash mid-append) like the transcript store", () => {
    const dirs = tmpDirs();
    const groupId = newGroupId();
    appendPoolSpend(dirs, groupId, entry({ approvalId: "req-ok" }));
    // Simulate a crash mid-append: a truncated JSON line.
    const path = join(dirs[0]!, groupId, "ledger.jsonl");
    writeFileSync(path, readFileSync(path, "utf8") + '{"id":"ps-torn","amountC');
    const ledger = readPoolLedger(dirs, groupId);
    expect(ledger).toHaveLength(1);
    expect(ledger[0]!.approvalId).toBe("req-ok");
  });

  it("an untouched group reads an empty ledger", () => {
    const dirs = tmpDirs();
    expect(readPoolLedger(dirs, newGroupId())).toEqual([]);
  });
});
