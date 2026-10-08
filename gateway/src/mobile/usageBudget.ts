/**
 * B2 — `shannon/usage.budget` read-only face: month-to-date spend + the
 * user-set monthly budget, mirroring the desktop's Usage governance numbers
 * (`desktop/src/usage_governance.rs`):
 *
 *  - monthCostUsd sums `cost_usd` over the JSONL records of
 *    `~/.shannon/usage.jsonl` whose `timestamp_ms` is at or after LOCAL
 *    midnight on the 1st of the current month (same local-calendar window
 *    convention as `summarize_windows`/`month_start_ms`); unparseable lines
 *    are skipped, and a missing/unreadable ledger fails open to 0.
 *  - budgetUsd reads `~/.shannon/desktop/config.json`, tolerating both key
 *    spellings (`monthly_budget_usd` — the serde field — or `monthlyBudgetUsd`);
 *    missing/non-numeric → null. Same file the desktop's budget card reads.
 *  - sessionCapUsd is ALWAYS null in v1: per-session caps live in the
 *    per-session sidecars, which this face deliberately does not read. The
 *    key stays on the wire as a forward-compat placeholder.
 *
 * Boundary (deliberate, keep in mind on every read): the ledger only records
 * DESKTOP engine sessions — gateway/mobile-side task spend is not written
 * there yet, so `monthCostUsd` is a LOWER bound of real spend, never an
 * overstatement. This face is read-only: no thresholds, no notifications, no
 * enforcement (the desktop owns all of that).
 */

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/** Default ledger: `~/.shannon/usage.jsonl` (desktop UsageStore append log). */
export function defaultLedgerPath(): string {
  return join(homedir(), ".shannon", "usage.jsonl");
}

/** Default config: `~/.shannon/desktop/config.json` (DesktopConfig). */
export function defaultDesktopConfigPath(): string {
  return join(homedir(), ".shannon", "desktop", "config.json");
}

/** The budget snapshot, as served by `shannon/usage.budget`. */
export interface UsageBudget {
  /** Local-calendar month key, `"%Y-%m"`. */
  month: string;
  /** Month-to-date USD spend (lower bound — see module doc). */
  monthCostUsd: number;
  /** Configured monthly budget; null when unset or not a usable number. */
  budgetUsd: number | null;
  /** Always null in v1 (forward-compat placeholder). */
  sessionCapUsd: null;
}

export interface UsageBudgetPaths {
  ledger: string;
  desktopConfig: string;
}

/** Default paths (real home); tests inject a tmp dir pair. */
export function defaultUsageBudgetPaths(): UsageBudgetPaths {
  return { ledger: defaultLedgerPath(), desktopConfig: defaultDesktopConfigPath() };
}

/**
 * Load the budget snapshot. Never throws — a missing ledger or config file
 * (and any unreadable/corrupt content) degrades to the honest zero/null
 * snapshot, exactly like the desktop's fail-open governance reads.
 */
export function loadUsageBudget(paths?: Partial<UsageBudgetPaths>): UsageBudget {
  const resolved = { ...defaultUsageBudgetPaths(), ...paths };
  const now = new Date();
  return {
    month: `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`,
    monthCostUsd: monthCost(resolved.ledger, now),
    budgetUsd: readBudget(resolved.desktopConfig),
    sessionCapUsd: null,
  };
}

/**
 * Epoch-ms of LOCAL midnight on the 1st of `now`'s month — the JS twin of
 * `usage_governance.rs::month_start_ms` (local calendar, fail-open to 0 when
 * the date construction misbehaves).
 */
export function monthStartMs(now: Date): number {
  const start = new Date(now.getFullYear(), now.getMonth(), 1, 0, 0, 0, 0);
  const ms = start.getTime();
  return Number.isFinite(ms) ? ms : 0;
}

/** Sum of this month's `cost_usd` over the ledger; 0 when unreadable. */
function monthCost(ledgerPath: string, now: Date): number {
  let content: string;
  try {
    content = readFileSync(ledgerPath, "utf8");
  } catch {
    return 0;
  }
  const start = monthStartMs(now);
  let total = 0;
  for (const line of content.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed.length === 0) continue;
    try {
      const rec = JSON.parse(trimmed) as { timestamp_ms?: unknown; cost_usd?: unknown };
      if (typeof rec.timestamp_ms !== "number" || typeof rec.cost_usd !== "number") continue;
      if (rec.timestamp_ms >= start) total += rec.cost_usd;
    } catch {
      continue; // unparseable line → skip, never fail the aggregate
    }
  }
  return total;
}

/**
 * The configured monthly budget: `monthly_budget_usd` preferred, the
 * camelCase `monthlyBudgetUsd` tolerated; absent or non-numeric (incl.
 * NaN/±Infinity) → null. Read-only — this face never writes the config.
 */
function readBudget(configPath: string): number | null {
  let content: string;
  try {
    content = readFileSync(configPath, "utf8");
  } catch {
    return null;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const cfg = parsed as { monthly_budget_usd?: unknown; monthlyBudgetUsd?: unknown };
  const raw = cfg.monthly_budget_usd ?? cfg.monthlyBudgetUsd;
  return typeof raw === "number" && Number.isFinite(raw) ? raw : null;
}
