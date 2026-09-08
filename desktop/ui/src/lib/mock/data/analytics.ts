// Analytics mock data: inbox items, OPC metrics, perf.
import type {
  InboxItem,
  OpcMetrics,
} from '@/types'

const now = Date.now()
const day = 86400_000
const dayIso = (n: number) => new Date(now - n * day).toISOString().slice(0, 10)

// Inbox (P0-3) — camelCase rows exactly as the SQLite store serialises them.
// `sessionId` values link to MOCK_SESSIONS so "continue in session" works in
// the demo; two items are `pending` with one carrying an `error` so the
// visual audit shows every card state (pending dot, error expander, rerun
// enabled/disabled).
export const MOCK_INBOX_ITEMS: InboxItem[] = [
  {
    id: 1,
    source: 'scheduled_task',
    sourceId: 'sched-002',
    sessionId: null,
    title: 'Nightly dependency audit failed',
    summary: 'cargo audit exited with code 2 after scanning 412 crates.',
    error: 'cargo audit exited with code 2: 2 vulnerabilities found\n  RUSTSEC-2026-0121 (regex) — upgrade to >= 1.11\n  RUSTSEC-2026-0044 (reqwest) — upgrade to >= 0.12.9',
    status: 'pending',
    createdAtMs: now - 2 * 3600_000,
    updatedAtMs: now - 2 * 3600_000,
  },
  {
    id: 2,
    source: 'routine',
    sourceId: 'sched-001',
    sessionId: 'sess-006',
    title: 'Weekly metrics digest finished',
    summary: 'Compiled Mon 9:02am. 3 anomalies flagged in the billing pipeline; digest posted to #ops.',
    error: null,
    status: 'pending',
    createdAtMs: now - 5 * 3600_000,
    updatedAtMs: now - 5 * 3600_000,
  },
  {
    id: 3,
    source: 'trigger',
    sourceId: 'ops-webhook',
    sessionId: null,
    title: 'Ops webhook: incident IR-2041 opened',
    summary: 'Trigger endpoint received a signed payload (HMAC verified): "checkout error rate 4.1%".',
    error: null,
    status: 'read',
    createdAtMs: now - 9 * 3600_000,
    updatedAtMs: now - 8 * 3600_000,
  },
  {
    id: 4,
    source: 'goal',
    sourceId: 'goal-billing-migration',
    sessionId: 'sess-008',
    title: 'Goal checkpoint: billing migration 60% complete',
    summary: 'Objective "migrate billing schema to v2" — 6 iterations, no stalls, budget on track.',
    error: null,
    status: 'read',
    createdAtMs: now - day + 3600_000,
    updatedAtMs: now - day + 3600_000,
  },
  {
    id: 5,
    source: 'routine',
    sourceId: 'sched-003',
    sessionId: 'sess-005',
    title: 'Investor update draft ready',
    summary: "Draft compiled from this week's sessions and metrics; ready for review.",
    error: null,
    status: 'read',
    createdAtMs: now - 2 * day,
    updatedAtMs: now - 2 * day,
  },
  {
    id: 6,
    source: 'routine',
    sourceId: 'sched-001',
    sessionId: 'sess-006',
    title: 'Weekly metrics digest finished',
    summary: 'Compiled Mon 9:00am. No anomalies detected.',
    error: null,
    status: 'archived',
    createdAtMs: now - 8 * day,
    updatedAtMs: now - 7 * day,
  },
]

export const MOCK_OPC_METRICS: OpcMetrics = {
  total: 16,
  completion_rate: 27.0,
  by_status: [
    { status: 'in_progress', count: 4 },
    { status: 'pending', count: 3 },
    { status: 'queued', count: 2 },
    { status: 'blocked', count: 2 },
    { status: 'completed', count: 4 },
    { status: 'failed', count: 2 },
  ],
  by_priority: [
    { priority: 'critical', count: 4 },
    { priority: 'high', count: 5 },
    { priority: 'normal', count: 5 },
    { priority: 'low', count: 2 },
  ],
  by_assignee: [
    { assignee: 'aurora', total: 6, done: 2, in_progress: 3 },
    { assignee: 'orion', total: 4, done: 0, in_progress: 1 },
    { assignee: 'nova', total: 4, done: 1, in_progress: 2 },
    { assignee: 'priya', total: 2, done: 0, in_progress: 1 },
  ],
  daily: [
    { date: dayIso(6), created: 3, completed: 2 },
    { date: dayIso(5), created: 4, completed: 1 },
    { date: dayIso(4), created: 2, completed: 3 },
    { date: dayIso(3), created: 5, completed: 2 },
    { date: dayIso(2), created: 3, completed: 4 },
    { date: dayIso(1), created: 2, completed: 1 },
    { date: dayIso(0), created: 4, completed: 2 },
  ],
} as unknown as OpcMetrics

// Perf traces (for /perf page)
export const MOCK_PERF_TRACES = [
  {
    name: 'list_tasks',
    p50_ms: 12,
    p95_ms: 28,
    p99_ms: 64,
    calls: 1240,
    error_rate: 0.001,
  },
  {
    name: 'list_scheduled_tasks',
    p50_ms: 18,
    p95_ms: 45,
    p99_ms: 120,
    calls: 412,
    error_rate: 0,
  },
  {
    name: 'send_message',
    p50_ms: 320,
    p95_ms: 1240,
    p99_ms: 3400,
    calls: 87,
    error_rate: 0.011,
  },
  {
    name: 'apply_diff',
    p50_ms: 45,
    p95_ms: 180,
    p99_ms: 420,
    calls: 156,
    error_rate: 0.006,
  },
  {
    name: 'lsp_code_actions',
    p50_ms: 28,
    p95_ms: 95,
    p99_ms: 240,
    calls: 89,
    error_rate: 0,
  },
  {
    name: 'get_opc_metrics',
    p50_ms: 8,
    p95_ms: 14,
    p99_ms: 32,
    calls: 312,
    error_rate: 0,
  },
  {
    name: 'list_inbox_items',
    p50_ms: 22,
    p95_ms: 68,
    p99_ms: 180,
    calls: 420,
    error_rate: 0.002,
  },
]

// LSP diagnostics for /quickfix
export const MOCK_DIAGNOSTICS = {
  files: [
    {
      path: 'src/billing/invoice.rs',
      language_id: 'rust',
      messages: [
        {
          line: 42,
          column: 5,
          severity: 'error',
          message: 'cannot find value `customer_balance` in this scope',
          source: 'rustc',
          code: 'E0425',
        },
        {
          line: 87,
          column: 14,
          severity: 'warning',
          message: 'unused variable: `tax_rate`',
          source: 'rustc',
          code: 'unused_variables',
        },
        {
          line: 124,
          column: 9,
          severity: 'warning',
          message: 'this `match` can be collapsed',
          source: 'clippy',
          code: 'clippy::collapsible_match',
        },
      ],
    },
    {
      path: 'src/webhooks/stripe.rs',
      language_id: 'rust',
      messages: [
        {
          line: 18,
          column: 1,
          severity: 'error',
          message: 'expected `,` or `}` after struct field',
          source: 'rustc',
          code: 'E0725',
        },
      ],
    },
    {
      path: 'src/api/routes.rs',
      language_id: 'rust',
      messages: [
        {
          line: 312,
          column: 5,
          severity: 'warning',
          message: 'function `legacy_handler` is never used',
          source: 'rustc',
          code: 'dead_code',
        },
      ],
    },
  ],
}

export const MOCK_CODE_ACTIONS = [
  {
    title: 'Collapse match arms',
    kind: 'refactor.rewrite',
    is_preferred: true,
  },
  {
    title: 'Remove unused variable `tax_rate`',
    kind: 'quickfix',
    is_preferred: false,
  },
  {
    title: 'Generate `customer_balance` field',
    kind: 'quickfix',
    is_preferred: false,
  },
]

export const MOCK_GOALS = [
  {
    id: 'goal-001',
    title: 'Ship pricing page redesign',
    description: 'Reduce pricing-page bounce rate from 38% to <25% via clearer copy and visual hierarchy.',
    status: 'in_progress',
    progress: 65,
    due_date: new Date(now + 10 * day).toISOString(),
    owner: 'nova',
    key_results: [
      { id: 'kr-1', text: 'Get 5 user tests with positive response', progress: 80, target: 5, current: 4 },
      { id: 'kr-2', text: 'Reduce bounce rate to <25%', progress: 40, target: 25, current: 32 },
      { id: 'kr-3', text: 'Ship to production', progress: 0, target: 1, current: 0 },
    ],
  },
  {
    id: 'goal-002',
    title: 'Cut billing-related support tickets by 50%',
    description: 'Schema v2 + self-serve refund flow. Baseline: 142 tickets/week.',
    status: 'in_progress',
    progress: 30,
    due_date: new Date(now + 30 * day).toISOString(),
    owner: 'orion',
    key_results: [
      { id: 'kr-1', text: 'Ship schema v2 migration', progress: 30, target: 1, current: 0 },
      { id: 'kr-2', text: 'Build self-serve refund', progress: 0, target: 1, current: 0 },
      { id: 'kr-3', text: 'Reduce tickets to <71/week', progress: 0, target: 71, current: 142 },
    ],
  },
  {
    id: 'goal-003',
    title: 'Launch 5 OAuth integration partners',
    description: 'Build OAuth app gallery with launch partners (Slack, Notion, Linear, Figma, Asana).',
    status: 'at_risk',
    progress: 40,
    due_date: new Date(now + 14 * day).toISOString(),
    owner: 'aurora',
    key_results: [
      { id: 'kr-1', text: 'Sign 5 partner agreements', progress: 60, target: 5, current: 3 },
      { id: 'kr-2', text: 'Ship OAuth scaffolding', progress: 100, target: 1, current: 1 },
      { id: 'kr-3', text: 'Launch gallery publicly', progress: 0, target: 1, current: 0 },
    ],
  },
  {
    id: 'goal-004',
    title: 'Improve onboarding activation rate',
    description: 'Take new-user activation from 41% to 55% via tour + templates.',
    status: 'in_progress',
    progress: 70,
    due_date: new Date(now + 7 * day).toISOString(),
    owner: 'nova',
    key_results: [
      { id: 'kr-1', text: 'A/B test onboarding tour', progress: 100, target: 1, current: 1 },
      { id: 'kr-2', text: 'Ship 3 starter templates', progress: 100, target: 3, current: 3 },
      { id: 'kr-3', text: 'Hit 55% activation', progress: 78, target: 55, current: 51 },
    ],
  },
  {
    id: 'goal-005',
    title: 'Q3 — Quarterly investor update',
    description: 'Send quarterly update with metrics, milestones, asks. Due Sep 30.',
    status: 'not_started',
    progress: 0,
    due_date: new Date(now + 90 * day).toISOString(),
    owner: 'aurora',
    key_results: [
      { id: 'kr-1', text: 'Compile metrics', progress: 0, target: 1, current: 0 },
      { id: 'kr-2', text: 'Draft narrative', progress: 0, target: 1, current: 0 },
      { id: 'kr-3', text: 'Send to investors', progress: 0, target: 30, current: 0 },
    ],
  },
]
