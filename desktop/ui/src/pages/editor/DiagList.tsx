// DiagList — renders the flat list of mixed (auto + manual) diagnostics with
// per-item actions: open quick-fix drawer, ask AI. Returns null when the
// list is empty.

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import type { EditorDiagnostic } from '@/components/editor/CodeEditor'
import type { MixedDiagnostic } from './types'

interface DiagListProps {
  t: (id: string, values?: Record<string, string | number | boolean>) => string
  diags: MixedDiagnostic[]
  onSquiggleClick: (diag: EditorDiagnostic) => void
  onAskAi: (d: MixedDiagnostic) => void
}

export default function DiagList({
  t,
  diags,
  onSquiggleClick,
  onAskAi,
}: DiagListProps) {
  if (diags.length === 0) return null
  return (
    <div className="bg-surface-container-lowest rounded-2xl p-md border border-outline-variant/30 shadow-sm">
      <h3 className="font-label-md text-on-surface mb-sm">{t('editor.diagnosticsList')}</h3>
      <ul className="flex flex-col gap-xs">
        {diags.map((d, i) => (
          <li key={i}>
            <Button
              type="button"
              variant="outline"
              onClick={() => onSquiggleClick(d)}
              className={cn(
                'h-auto w-full text-left flex items-start gap-sm px-sm py-sm rounded-lg border border-outline-variant/30 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 cursor-pointer whitespace-normal',
              )}
            >
              <span
                className="font-label-sm uppercase text-[10px] mt-0.5 tracking-wider"
                style={{
                  color:
                    d.severity === 'error'
                      ? 'var(--color-error)'
                      : d.severity === 'warning'
                        ? 'var(--color-tertiary)'
                        : 'var(--color-on-surface-variant)',
                }}
              >
                {t(`editor.severity.${d.severity}`)}
              </span>
              <span className="flex-1 font-label-md">
                <span className="font-mono text-on-surface-variant">
                  {d.start_line + 1}:{d.start_character + 1}
                </span>{' '}
                {d.message}
              </span>
              {d.kind === 'auto' ? (
                <span
                  className="font-label-sm uppercase text-[10px] tracking-wider text-on-surface-variant"
                  title={
                    d.source
                      ? (d.code
                          ? t('editor.sourceTitle', { source: d.source, code: d.code })
                          : t('editor.sourceTitle.noCode', { source: d.source }))
                      : t('editor.source')
                  }
                >
                  {d.source ?? t('editor.source')}
                </span>
              ) : (
                <span className="font-label-sm uppercase text-[10px] tracking-wider text-on-surface-variant">
                  {t('editor.manual')}
                </span>
              )}
              <span className="material-symbols-outlined text-[14px] text-primary">
                build
              </span>
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={() => onAskAi(d)}
              aria-label={t('editor.askAi')}
              title={t('editor.askAi')}
              className="flex items-center gap-xs px-xs py-0.5 rounded-full border border-outline-variant/40 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 cursor-pointer"
            >
              <span className="material-symbols-outlined text-[14px] text-primary">
                chat
              </span>
              <span className="font-label-sm">{t('editor.askAi')}</span>
            </Button>
          </li>
        ))}
      </ul>
    </div>
  )
}