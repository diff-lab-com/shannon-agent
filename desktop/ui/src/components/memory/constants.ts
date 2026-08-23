// Memory panel — shared constants and lookup tables. Extracted from
// MemoryPanel.tsx (T3.1) so the orchestrator and its sub-components can
// share a single source of truth for category filters and visual styling.
import type { MemoryCategory } from '@/lib/tauri-api'

export type CategoryFilter = MemoryCategory | 'all'

export const CATEGORIES: CategoryFilter[] = [
  'all',
  'preference',
  'pattern',
  'decision',
  'error',
  'context',
]

export const CATEGORY_ICON: Record<MemoryCategory, string> = {
  preference: 'tune',
  pattern: 'pattern',
  decision: 'fork_right',
  error: 'bug_report',
  context: 'lightbulb',
}

export const CATEGORY_COLOR: Record<MemoryCategory, string> = {
  preference: 'bg-primary-container/50 text-on-primary-container',
  pattern: 'bg-secondary-container/50 text-on-secondary-container',
  decision: 'bg-tertiary-container/50 text-on-tertiary-container',
  error: 'bg-error-container/50 text-on-error-container',
  context: 'bg-surface-container-high text-on-surface',
}
