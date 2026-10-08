import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { createConsoleLogger } from "../../logger.js";
import { loadUsageBudget, monthStartMs } from "../usageBudget.js";
import { createEngineHandlers } from "../engineBridge.js";

/**
 * B2 acceptance: `shannon/usage.budget` — month-to-date aggregation over the
 * desktop ledger (local-calendar month window, bad lines skipped, missing
 * files fail open), the budget key double-spelling, and the handler surface.
 */

const logger = createConsoleLogger("error");

let tmpDirs: string[] = [];
afterEach(() => {
  for (const dir of tmpDirs) rmSync(dir, { recursive: true, force: true });
  tmpDirs = [];
});

function tmpPaths(): { ledger: string; desktopConfig: string; root: string } {
  const root = mkdtempSync(join(tmpdir(), "gw-budget-"));
  tmpDirs.push(root);
  return { ledger: join(root, "usage.jsonl"), desktopConfig: join(root, "config.json"), root };
}

function ledgerLine(timestampMs: number, costUsd: number, extra = ""): string {
  return `${JSON.stringify({ timestamp_ms: timestampMs, cost_usd: costUsd, model: "m" })}${extra}`;
}

describe("loadUsageBudget", () => {
  it("sums only the current local-calendar month (window inclusive at month start)", () => {
    const paths = tmpPaths();
    const now = new Date();
    const start = monthStartMs(now);
    const day = 86_400_000;
    const lines = [
      ledgerLine(start, 1.25), // boundary: local midnight day 1 counts
      ledgerLine(Date.now(), 0.75), // today counts
      ledgerLine(start - day, 9.99), // last month: excluded
      ledgerLine(start - 40 * day, 5.0), // far past: excluded
    ].join("\n");
    writeFileSync(paths.ledger, lines, "utf8");

    const snap = loadUsageBudget(paths);
    expect(snap.monthCostUsd).toBeCloseTo(2.0, 9);
    expect(snap.month).toBe(
      `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`,
    );
    expect(snap.sessionCapUsd).toBeNull();
  });

  it("skips unparseable / non-numeric lines instead of failing the aggregate", () => {
    const paths = tmpPaths();
    writeFileSync(
      paths.ledger,
      [
        ledgerLine(Date.now(), 0.5, ',"trailing":true'),
        "this is not json",
        '{"timestamp_ms":"oops","cost_usd":1}',
        '{"cost_usd":3.5}',
        ledgerLine(Date.now(), 1.5),
        "", // trailing blank line
      ].join("\n"),
      "utf8",
    );

    const snap = loadUsageBudget(paths);
    // Only the final intact line (1.5) counts — the trailing-garbage line,
    // the non-JSON line and the non-numeric records are all skipped.
    expect(snap.monthCostUsd).toBeCloseTo(1.5, 9);
  });

  it("an empty ledger file yields zero; a missing one fails open to zero too", () => {
    const empty = tmpPaths();
    writeFileSync(empty.ledger, "", "utf8");
    expect(loadUsageBudget(empty).monthCostUsd).toBe(0);

    const missing = tmpPaths();
    const snap = loadUsageBudget(missing);
    expect(snap.monthCostUsd).toBe(0);
    expect(snap.budgetUsd).toBeNull();
    expect(snap.month).toMatch(/^\d{4}-\d{2}$/);
  });

  it("budgetUsd tolerates both key spellings and rejects non-numbers", () => {
    const snake = tmpPaths();
    writeFileSync(snake.desktopConfig, JSON.stringify({ monthly_budget_usd: 2000 }), "utf8");
    writeFileSync(snake.ledger, ledgerLine(Date.now(), 37.42), "utf8");
    expect(loadUsageBudget(snake)).toMatchObject({
      monthCostUsd: 37.42,
      budgetUsd: 2000,
      sessionCapUsd: null,
    });

    const camel = tmpPaths();
    writeFileSync(camel.desktopConfig, JSON.stringify({ monthlyBudgetUsd: 12.5 }), "utf8");
    expect(loadUsageBudget(camel).budgetUsd).toBe(12.5);

    for (const bad of [
      JSON.stringify({ monthly_budget_usd: "lots" }),
      JSON.stringify({ monthlyBudgetUsd: null }),
      "{}",
      "{corrupt",
    ]) {
      const p = tmpPaths();
      writeFileSync(p.desktopConfig, bad, "utf8");
      expect(loadUsageBudget(p).budgetUsd).toBeNull();
    }
  });

  it("a missing budget key is null while a present ledger still aggregates", () => {
    const paths = tmpPaths();
    writeFileSync(paths.ledger, ledgerLine(Date.now(), 2.25), "utf8");
    writeFileSync(paths.desktopConfig, JSON.stringify({ theme: "dark" }), "utf8");

    expect(loadUsageBudget(paths)).toEqual({
      month: expect.stringMatching(/^\d{4}-\d{2}$/),
      monthCostUsd: 2.25,
      budgetUsd: null,
      sessionCapUsd: null,
    });
  });
});

describe("shannon/usage.budget via engineBridge", () => {
  it("serves the snapshot over the RPC surface (gated, never errors)", async () => {
    const paths = tmpPaths();
    writeFileSync(paths.ledger, ledgerLine(Date.now(), 3.5), "utf8");
    writeFileSync(
      paths.desktopConfig,
      JSON.stringify({ monthly_budget_usd: 100 }),
      "utf8",
    );

    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      usageBudgetPaths: paths,
    });
    const ctx = { sessionId: null, logger } as any;
    const res = await handlers["shannon/usage.budget"]!({}, ctx);
    expect(res).toMatchObject({
      kind: "result",
      result: { monthCostUsd: 3.5, budgetUsd: 100, sessionCapUsd: null },
    });
  });
});
