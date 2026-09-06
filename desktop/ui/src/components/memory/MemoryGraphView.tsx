// Memory graph view (P2-4) — self-implemented SVG visualization of the
// project → category → entries memory graph. No graph-library dependency:
// the layout is a deterministic clustered arrangement (see graphUtils.ts)
// with ≤60 static force-relaxation iterations and no continuous animation.
//
// A11y: the svg is a single role="img" with an aria-label (200 focusable
// nodes would be noise); the always-present list view is the accessible
// alternative, per the brief. The detail side panel is regular HTML.
//
// Edge semantics (conservative): solid `cluster` lines are pure containment
// (project root → category cluster → its entries); dashed `session` lines are
// weak associations between entries captured in the same source session
// (chained in creation order). Nothing else implies relatedness.

import { useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import type { MemoryGraph, MemoryGraphNode } from '@/lib/tauri-api'
import { categoryColor, filterMemoryGraph, layoutMemoryGraph } from './graphUtils'
import { cn } from '@/lib/utils'

interface MemoryGraphViewProps {
  graph: MemoryGraph
  loading?: boolean
  /** Active category filter ('all' | category) — applied client-side. */
  category: string
  /** Active free-text query — applied client-side on entry labels. */
  query: string
  /** Jump handler (Memory page wires it to get_memory_source + switchSession). */
  onOpenMemorySource?: (memoryId: string, sourceSessionId: string) => void
}

const VIEW_WIDTH = 900

export function MemoryGraphView({
  graph,
  loading,
  category,
  query,
  onOpenMemorySource,
}: MemoryGraphViewProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const [selectedId, setSelectedId] = useState<string | null>(null)

  const filtered = useMemo(() => filterMemoryGraph(graph, category, query), [graph, category, query])
  const layout = useMemo(() => layoutMemoryGraph(filtered, VIEW_WIDTH), [filtered])
  const byId = useMemo(
    () => new Map(layout.nodes.map((n) => [n.node.id, n])),
    [layout],
  )

  const selected: MemoryGraphNode | null = useMemo(() => {
    if (!selectedId) return null
    return byId.get(selectedId)?.node ?? null
  }, [selectedId, byId])

  if (loading) {
    return (
      <div className="text-center py-3xl text-on-surface-variant" data-testid="memory-graph-loading">
        {t('memory.loading')}
      </div>
    )
  }

  if (filtered.nodes.length === 0 || graph.entryCount === 0) {
    return (
      <div className="text-center py-3xl" data-testid="memory-graph-empty">
        <span className="material-symbols-outlined icon-2xl text-on-surface-variant/40 mb-md block">
          hub
        </span>
        <p className="text-on-surface-variant mb-sm">{t('memory.graph.empty')}</p>
        <p className="text-body-sm text-on-surface-variant/70">{t('memory.graph.emptyHint')}</p>
      </div>
    )
  }

  return (
    <div className="relative">
      {graph.truncated && (
        <div
          role="status"
          className="flex items-center gap-sm px-md py-sm mb-md rounded-xl bg-tertiary-container/20 border border-tertiary-container/40 text-on-surface text-label-md"
        >
          <span className="material-symbols-outlined text-[18px]">info</span>
          {t('memory.graph.truncated', { count: graph.entryCount, shown: graph.maxEntries })}
        </div>
      )}

      <div className="flex flex-col lg:flex-row gap-md items-start">
        <div className="flex-1 min-w-0 w-full rounded-xl border border-outline-variant/30 bg-surface-container-lowest overflow-hidden">
          <svg
            role="img"
            aria-label={t('memory.graph.ariaLabel', { count: graph.entryCount })}
            viewBox={`0 0 ${layout.width} ${layout.height}`}
            className="block w-full h-auto"
            data-testid="memory-graph-svg"
          >
            {layout.edges.map((e) => (
              <line
                key={`${e.kind}:${e.source}->${e.target}`}
                x1={e.x1}
                y1={e.y1}
                x2={e.x2}
                y2={e.y2}
                stroke={
                  e.kind === 'session'
                    ? 'var(--color-primary)'
                    : 'var(--color-outline-variant)'
                }
                strokeWidth={e.kind === 'session' ? 1.4 : 1.6}
                strokeDasharray={e.kind === 'session' ? '4 3' : undefined}
                opacity={e.kind === 'session' ? 0.55 : 1}
                data-edge-kind={e.kind}
              />
            ))}
            {layout.nodes.map((n) => {
              const isSelected = n.node.id === selectedId
              const color =
                n.node.kind === 'project'
                  ? 'var(--color-inverse-surface)'
                  : categoryColor(n.node.category)
              return (
                <g
                  key={n.node.id}
                  onClick={() => setSelectedId(n.node.kind === 'entry' ? n.node.id : null)}
                  className={n.node.kind === 'entry' ? 'cursor-pointer' : undefined}
                  data-node-kind={n.node.kind}
                  data-node-id={n.node.id}
                >
                  {n.node.kind === 'entry' && <title>{n.node.label}</title>}
                  <circle
                    cx={n.x}
                    cy={n.y}
                    r={n.radius}
                    fill={color}
                    fillOpacity={n.node.kind === 'entry' ? 0.85 : 1}
                    stroke={isSelected ? 'var(--color-primary)' : 'var(--color-surface-container-lowest)'}
                    strokeWidth={isSelected ? 3 : 1.5}
                  />
                  {n.node.kind !== 'entry' && (
                    <text
                      x={n.x}
                      y={n.y + 4}
                      textAnchor="middle"
                      fontSize={n.node.kind === 'project' ? 11 : 10}
                      fill={
                        n.node.kind === 'project'
                          ? 'var(--color-on-primary)'
                          : 'var(--color-surface-container-lowest)'
                      }
                    >
                      {n.node.kind === 'project' ? n.node.label.split(/[\\/]/).pop() : n.node.label}
                    </text>
                  )}
                </g>
              )
            })}
          </svg>
        </div>

        {selected && (
          <aside
            className="w-full lg:w-[300px] shrink-0 rounded-xl border border-outline-variant/30 bg-surface-container-low p-md"
            data-testid="memory-graph-detail"
            aria-label={t('memory.graph.detail.title')}
          >
            <div className="flex items-start justify-between gap-sm mb-sm">
              <span className="text-label-xs px-sm py-[2px] rounded-full bg-surface-container-high text-on-surface-variant font-bold uppercase">
                {t(`memory.category.${selected.category ?? 'context'}`)}
              </span>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => setSelectedId(null)}
                aria-label={t('memory.graph.detail.close')}
                className="rounded-lg hover:bg-surface-container-high"
              >
                <span className="material-symbols-outlined text-[18px] text-on-surface-variant">
                  close
                </span>
              </Button>
            </div>
            <p className="text-body-md text-on-surface whitespace-pre-wrap break-words mb-sm">
              {selected.label}
            </p>
            <p className="text-label-sm text-on-surface-variant mb-sm">
              {t('memory.graph.detail.confidence')}:{' '}
              {Math.round(Math.max(0, Math.min(selected.weight, 1)) * 100)}%
            </p>
            {selected.sourceSessionId && (
              <div className="flex flex-wrap items-center gap-sm">
                <span
                  className={cn(
                    'text-label-xs px-sm py-[2px] rounded-full bg-primary-container/40 text-on-surface',
                  )}
                  data-testid="memory-graph-source-badge"
                >
                  {selected.sourceKind
                    ? t(`memory.source.kind.${selected.sourceKind}`)
                    : t('memory.source.badge', { session: shortSession(selected.sourceSessionId) })}
                  {selected.sourceKind ? ` · ${shortSession(selected.sourceSessionId)}` : ''}
                </span>
                {onOpenMemorySource && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() =>
                      onOpenMemorySource(selected.id.replace(/^entry:/, ''), selected.sourceSessionId as string)
                    }
                  >
                    <span className="material-symbols-outlined text-[16px] mr-xs">chat</span>
                    {t('memory.source.jump')}
                  </Button>
                )}
              </div>
            )}
          </aside>
        )}
      </div>
    </div>
  )
}

function shortSession(id: string): string {
  return id.length > 10 ? `${id.slice(0, 8)}…` : id
}
