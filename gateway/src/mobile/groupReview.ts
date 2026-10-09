/**
 * §S B6.2 aggregation face — the deterministic, host-side aggregators behind
 * the 16-screen review (`aggregateGroupReview`) and the R11 group daily report
 * (`buildDailyReport` + `createDailyReportTimer`), plus the per-minute tick
 * timer that fires the report at a group's configured local
 * `rules.dailyReportAt`.
 *
 * Honesty posture (§B6.2): every number is read from the group store's single
 * sources — `group.json` (pool/member fields; B6.1's settle chain feeds them)
 * and the transcript — and anything v1 cannot know is an honest 0 / omitted
 * key, never an estimate:
 *  - `decisionCount` is always 0 (no approval-attribution counting face yet);
 *  - `todaySpentCny` is always 0 — the PER-DAY pool ledger face is not built
 *    (B6.1 supplies the pool cumulative, see `review.pool.spentCny`); the
 *    report says so in text instead of inventing a number;
 *  - `handoffCount` counts transcript entries carrying the host-local
 *    `systemKind: "handoff"` marker — pre-B6.2 transcripts have no marker and
 *    undercount honestly;
 *  - a report whose generation throws is skipped for the day (当日报缺不补发,
 *    same as a host that was offline at the report minute).
 *
 * The timer only runs while at least one ACTIVE group has `dailyReportAt`
 * configured (lazy arm / self-disarm), stamps each (group, day) attempt in
 * memory so one minute can never double-fire, and is stopped via the host
 * shutdown chain (`createGroupHandlers(...).stop()` in bootstrap /
 * dev-standalone). It is deliberately NOT in the §O3 wake whitelist — a daily
 * report is non-interactive (notification-tier, no ring).
 */

import type { Logger } from "../adapters/types.js";
import type { MobileDispatchHub } from "./hub.js";
import type {
  GroupReview,
  GroupReviewDeliverable,
  GroupReviewLine,
  ShannonEvent,
} from "./protocol.js";
import {
  appendTranscript,
  listGroups,
  readTranscript,
  saveGroup,
  type GroupRecord,
  type GroupTranscriptEntry,
} from "./groupStore.js";

/** The daily-report wire payload (the `report` key of `group.report`). */
export type GroupReportPayload = Extract<ShannonEvent, { type: "group.report" }>["report"];

// ── local-time helpers (the report contract is LOCAL timezone) ─────────────

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

/** Local-calendar day key `YYYY-MM-DD`. */
export function localDateKey(d: Date): string {
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

/** Local wall-clock `HH:mm`. */
export function localHHmm(d: Date): string {
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

function clip(text: string, max: number): string {
  return text.length <= max ? text : `${text.slice(0, max)}…`;
}

function firstNonEmptyLine(text: string): string | null {
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (trimmed.length > 0) return trimmed;
  }
  return null;
}

// ── review aggregate (16 复盘面, computed once at archive time) ─────────────

/**
 * Aggregate the review from the group's AS-LIVED state. Callers must invoke
 * this BEFORE flipping member statuses to archived (archive handler order):
 * `goals.done` counts members whose status is "done" at this instant. Pure —
 * throws only if the caller hands it unusable data; the archive handler wraps
 * it so a broken transcript degrades to review-absent, never a failed archive.
 */
export function aggregateGroupReview(
  record: GroupRecord,
  transcript: GroupTranscriptEntry[],
): GroupReview {
  const members = record.members;
  const done = members.filter((m) => m.status === "done").length;

  const startMs = Date.parse(record.createdAt);
  const last = transcript.length > 0 ? transcript[transcript.length - 1] : undefined;
  const endMs = last ? Date.parse(last.ts) : NaN;
  const durationMinutes =
    Number.isFinite(startMs) && Number.isFinite(endMs) && endMs > startMs
      ? Math.floor((endMs - startMs) / 60_000)
      : 0;

  // Transcript-marked handoff cards. v1 honest lower bound: entries written
  // before the B6.2 `systemKind` marker exist carry none and don't count.
  const handoffCount = transcript.filter((e) => e.systemKind === "handoff").length;

  // v1 has no approval-attribution counting face (who approved what, per
  // member) — an honest 0, never an estimate (宁缺勿造; §B6.2 口径).
  const decisionCount = 0;

  const perMember = members
    .filter((m) => m.spentCny > 0)
    .map((m) => ({
      memberId: m.memberId,
      label: m.label,
      title: m.title,
      amountCny: Math.round(m.spentCny * 100) / 100,
    }));

  const deliverables: GroupReviewDeliverable[] = members
    .filter((m) => m.status === "done")
    .map((m) => {
      const lastOutput = [...transcript]
        .reverse()
        .find((e) => e.kind === "member" && e.member?.memberId === m.memberId);
      const firstLine = lastOutput ? firstNonEmptyLine(lastOutput.content) : null;
      const lines: GroupReviewLine[] = firstLine ? [{ k: "产出摘要", v: clip(firstLine, 80) }] : [];
      return {
        memberId: m.memberId,
        title: m.title,
        status: "已完成",
        lines,
        // R14 desktop deep link has no protocol yet — always null (the phone
        // renders the 「桌面深链 · 待协议」 note).
        artifact: null,
      };
    });

  return {
    goals: { done, total: members.length },
    durationMinutes,
    handoffCount,
    decisionCount,
    pool: {
      totalCny: record.pool.totalCny,
      spentCny: record.pool.spentCny,
      perMember,
    },
    deliverables,
  };
}

// ── daily report (R11, fired by the per-minute tick) ───────────────────────

/**
 * Compose the day's report from real aggregates only. `todaySpentCny` is the
 * honest v1 0 — the PER-DAY ledger face is not built (B6.1's settle chain
 * feeds the pool CUMULATIVE, `review.pool.spentCny`), and the text SAYS so:
 * the number is a declared lower bound, not a silent zero.
 */
export function buildDailyReport(
  record: GroupRecord,
  transcript: GroupTranscriptEntry[],
  nowMs: number,
): GroupReportPayload {
  const d = new Date(nowMs);
  const date = localDateKey(d);
  const done = record.members.filter((m) => m.status === "done").length;
  const total = record.members.length;
  const handoffsToday = transcript.filter(
    (e) =>
      e.systemKind === "handoff" &&
      localDateKey(new Date(Date.parse(e.ts))) === date,
  ).length;
  // Honest 0: per-day ledger face not built — never a fabricated number.
  const todaySpentCny = 0;
  const text = [
    `群日报 · ${date}`,
    `目标进度：${done}/${total}`,
    `今日交接：${handoffsToday} 次`,
    `今日池支出：¥${todaySpentCny.toFixed(2)}（按日账本面未做，池累计见复盘；此数为如实下界）`,
  ].join("\n");
  return {
    date,
    kind: "daily",
    text,
    goalProgress: { done, total },
    todaySpentCny,
    ts: new Date(nowMs).toISOString(),
  };
}

// ── the per-minute tick timer ────────────────────────────────────────────────

export interface DailyReportTimerOptions {
  hub: MobileDispatchHub;
  logger: Logger;
  /** Group storage roots (default `~/.shannon/groups`); mirrors groupHandlers. */
  groupsDirs?: string[];
  /** Test seam: the clock (defaults to Date.now). */
  now?: () => number;
  /**
   * Test seam: schedule the next tick (`fn` after `delayMs`); returns its
   * disposer. Defaults to a Node setTimeout. Tests inject a manual collector
   * and invoke the captured fn to simulate elapsed minutes.
   */
  scheduleNext?: (fn: () => void, delayMs: number) => () => void;
  /** Tick cadence in ms (default 60_000 — minute-aligned scheduling). */
  tickIntervalMs?: number;
}

export interface DailyReportTimer {
  /**
   * (Re)arm the tick chain — no-op when already armed, stopped, or when no
   * active group has `dailyReportAt` configured (仅在有 active 群配置了
   * dailyReportAt 时运行). Handlers call this after group create/archive so a
   * late-configured group starts being served without a restart.
   */
  arm(): void;
  /** Stop the chain (host shutdown). Idempotent; a stopped timer stays stopped. */
  stop(): void;
}

export function createDailyReportTimer(opts: DailyReportTimerOptions): DailyReportTimer {
  const dirs = opts.groupsDirs ?? [];
  const now = () => opts.now?.() ?? Date.now();
  const intervalMs = opts.tickIntervalMs ?? 60_000;
  const scheduleNext =
    opts.scheduleNext ??
    ((fn: () => void, delayMs: number) => {
      const t = setTimeout(fn, delayMs);
      t.unref?.();
      return () => clearTimeout(t);
    });

  let disposeNext: (() => void) | null = null;
  let stopped = false;
  /** groupId → local date key of the last (attempted) report — dedupe + the
   *  生成失败当日不补发 posture: an attempt is never retried that day. */
  const attempted = new Map<string, string>();

  const eligible = (): GroupRecord[] =>
    listGroups(dirs).filter(
      (r) => r.status === "active" && typeof r.rules?.dailyReportAt === "string",
    );

  function arm(): void {
    if (stopped || disposeNext !== null) return;
    if (eligible().length === 0) return;
    // Minute-aligned: land every tick on the :00 boundary so a matching
    // HH:mm can never be skipped by drift.
    disposeNext = scheduleNext(() => tick(), intervalMs - (now() % intervalMs));
  }

  function tick(): void {
    disposeNext?.();
    disposeNext = null;
    if (stopped) return;
    try {
      runDueReports();
    } catch (err) {
      opts.logger.warn(`daily report tick failed: ${(err as Error).message}`);
    }
    arm(); // self-disarm when the last eligible group is gone
  }

  function runDueReports(): void {
    const d = new Date(now());
    const hhmm = localHHmm(d);
    const dateKey = localDateKey(d);
    for (const record of eligible()) {
      if (record.rules.dailyReportAt !== hhmm) continue;
      if (attempted.get(record.groupId) === dateKey) continue;
      // Mark FIRST: a failed generation is today's missed report (缺报不补发),
      // not something the next tick within this minute may retry.
      attempted.set(record.groupId, dateKey);
      try {
        deliverReport(record, now());
      } catch (err) {
        opts.logger.warn(
          `daily report for ${record.groupId} failed — day stays report-less ` +
            `(不补发): ${(err as Error).message}`,
        );
      }
    }
  }

  function deliverReport(record: GroupRecord, nowMs: number): void {
    const transcript = readTranscript(dirs, record.groupId);
    const report = buildDailyReport(record, transcript, nowMs);
    opts.hub.broadcastEvent({ type: "group.report", session_id: record.groupId, report });
    // The thread keeps an honest replay copy (same posture as system cards).
    appendTranscript(dirs, record.groupId, {
      role: "assistant",
      content: report.text,
      ts: new Date(nowMs).toISOString(),
      kind: "system",
      systemKind: "daily-report",
    });
    record.lastActivityAt = new Date(nowMs).toISOString();
    saveGroup(dirs, record);
  }

  return {
    arm,
    stop() {
      stopped = true;
      disposeNext?.();
      disposeNext = null;
    },
  };
}
