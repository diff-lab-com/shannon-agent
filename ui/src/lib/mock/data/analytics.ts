// Analytics mock data: triage items, OPC metrics, perf, billing.
import type {
  TriageItem,
  TriageStats,
  OpcMetrics,
  BillingPlan,
  CostRecord,
  BillingHistory,
} from '@/types'

const now = Date.now()
const day = 86400_000
const dayIso = (n: number) => new Date(now - n * day).toISOString().slice(0, 10)

export const MOCK_TRIAGE_ITEMS: TriageItem[] = [
  {
    id: 'triage-001',
    title: 'Customer #4421 refund failed',
    detail: 'Stripe API timed out during refund. Customer waiting 4h.',
    severity: 'critical',
    source: 'support',
    created_at: Math.floor((now - 4 * 3600_000) / 1000),
    read: false,
    archived: false,
    action_label: 'Retry refund',
    action_target: 'task-015',
  } as unknown as TriageItem,
  {
    id: 'triage-002',
    title: 'Webhook latency spike — investigate',
    detail: 'P99 latency jumped 20x at 14:32 UTC. Ongoing.',
    severity: 'critical',
    source: 'monitoring',
    created_at: Math.floor((now - 2 * 3600_000) / 1000),
    read: false,
    archived: false,
    action_label: 'Open incident',
    action_target: 'task-013',
  } as unknown as TriageItem,
  {
    id: 'triage-003',
    title: 'PR #2841 needs your review',
    detail: 'Billing schema migration. Priya started review; 3 comments unresolved.',
    severity: 'high',
    source: 'github',
    created_at: Math.floor((now - 6 * 3600_000) / 1000),
    read: false,
    archived: false,
    action_label: 'Open PR',
    action_target: 'https://github.com/co/repo/pull/2841',
  } as unknown as TriageItem,
  {
    id: 'triage-004',
    title: '4 new enterprise security questionnaires',
    detail: '2 SOC2, 1 HIPAA, 1 ISO27001. Avg 3-day turnaround expected.',
    severity: 'high',
    source: 'sales',
    created_at: Math.floor((now - day) / 1000),
    read: true,
    archived: false,
    action_label: 'Start triage',
    action_target: 'task-009',
  } as unknown as TriageItem,
  {
    id: 'triage-005',
    title: 'Weekly metrics digest ready',
    detail: 'Generated Mon 9:02am. 3 anomalies flagged.',
    severity: 'normal',
    source: 'automation',
    created_at: Math.floor((now - 2 * day) / 1000),
    read: true,
    archived: false,
    action_label: 'View digest',
    action_target: 'sess-006',
  } as unknown as TriageItem,
  {
    id: 'triage-006',
    title: 'A/B test onboarding variant B is winning',
    detail: '+12% activation (p=0.03). Recommend ship.',
    severity: 'normal',
    source: 'experiment',
    created_at: Math.floor((now - 3 * day) / 1000),
    read: true,
    archived: false,
    action_label: 'View results',
    action_target: 'task-006',
  } as unknown as TriageItem,
  {
    id: 'triage-007',
    title: 'Stale PR #2801 open 9 days',
    detail: 'Feature/old-billing-refactor. Author @orion last active 5d ago.',
    severity: 'low',
    source: 'github',
    created_at: Math.floor((now - 4 * day) / 1000),
    read: true,
    archived: false,
    action_label: 'Close or ping',
    action_target: 'https://github.com/co/repo/pull/2801',
  } as unknown as TriageItem,
  {
    id: 'triage-008',
    title: 'Slack #support hit 50+ unread',
    detail: 'Mostly onboarding questions. Consider doc update.',
    severity: 'low',
    source: 'slack',
    created_at: Math.floor((now - 5 * day) / 1000),
    read: true,
    archived: true,
    action_label: 'Open Slack',
    action_target: 'slack://channel?id=support',
  } as unknown as TriageItem,
]

export const MOCK_TRIAGE_STATS: TriageStats = {
  total_open: 6,
  critical: 2,
  high: 2,
  normal: 2,
  low: 1,
  unread: 4,
  archived: 1,
} as unknown as TriageStats

export const MOCK_OPC_METRICS: OpcMetrics = {
  total: 16,
  completion_rate: 0.27,
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

export const MOCK_BILLING_PLAN: BillingPlan = {
  name: 'Pro',
  price: 24,
  token_limit: 2_000_000,
  features: [
    'Unlimited sessions',
    '5 concurrent agents',
    'Claude Sonnet + Opus access',
    'MCP marketplace',
    'Priority support',
  ],
}

export const MOCK_COST_HISTORY: CostRecord[] = Array.from({ length: 14 }).map((_, i) => {
  const base = 8 + Math.sin(i / 2) * 3
  const noise = (Math.random() - 0.5) * 2
  const cost = Math.max(2, base + noise)
  return {
    date: dayIso(13 - i),
    input_tokens: Math.floor(cost * 25000 + Math.random() * 5000),
    output_tokens: Math.floor(cost * 8000 + Math.random() * 2000),
    cost_usd: Math.round(cost * 100) / 100,
  }
})

export const MOCK_BILLING_HISTORY: BillingHistory[] = [
  { id: 'inv-2026-06', date: dayIso(0), description: 'Pro plan — June 2026', amount: 24, status: 'paid' },
  { id: 'inv-2026-05', date: dayIso(30), description: 'Pro plan — May 2026', amount: 24, status: 'paid' },
  { id: 'inv-2026-04', date: dayIso(60), description: 'Pro plan — April 2026', amount: 24, status: 'paid' },
  { id: 'inv-2026-03', date: dayIso(90), description: 'Pro plan — March 2026', amount: 24, status: 'paid' },
  { id: 'inv-2026-02', date: dayIso(120), description: 'Pro plan — February 2026 + overage', amount: 38, status: 'paid' },
  { id: 'inv-2026-01', date: dayIso(150), description: 'Pro plan — January 2026', amount: 24, status: 'paid' },
]

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
    name: 'list_triage_items',
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
