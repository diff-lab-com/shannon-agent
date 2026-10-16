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

// G1-B 语义四色 (设计 08 分类 chip): preference=紫 / decision=青 / pattern
// (project-fact 类)=灰 / context (insight, lightbulb)=绿 / error=红 — each
// category rides its semantic token dim pair instead of one flat grey chip.
export const CATEGORY_COLOR: Record<MemoryCategory, string> = {
  preference: 'bg-primary-container text-on-primary-container',
  pattern: 'bg-on-surface-variant/10 text-on-surface-variant',
  decision: 'bg-secondary-container text-on-secondary-container',
  error: 'bg-error-container text-on-error-container',
  context: 'bg-success-container text-on-success-container',
}
