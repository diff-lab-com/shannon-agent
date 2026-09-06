/**
 * P1-5 C-2 — the slim workspace controls row: preset switcher (聚焦聊天 /
 * 评审 / 构建), add-panel menu and the reset button.
 *
 * Rendered only in the main window above the grid. Kept deliberately slim
 * (one h-7 row, muted tokens) so the default-focus appearance stays
 * near-identical to the pre-workspace chat page.
 */
import { useCallback, useState } from 'react'
import { useIntl } from 'react-intl'
import { cn } from '@/lib/utils'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'
import {
  matchesPreset,
  PANEL_KINDS,
  type PanelKind,
  type PresetName,
  type WorkspaceLayout,
} from './layout'

interface WorkspaceToolbarProps {
  layout: WorkspaceLayout
  onPreset: (name: PresetName) => void
  onAdd: (kind: PanelKind) => void
  onReset: () => void
}

const PRESETS: { name: PresetName; labelId: string }[] = [
  { name: 'focus', labelId: 'workspace.preset.focus' },
  { name: 'review', labelId: 'workspace.preset.review' },
  { name: 'build', labelId: 'workspace.preset.build' },
]

const ADDABLE_KINDS: PanelKind[] = PANEL_KINDS.filter(k => k !== 'chat') as PanelKind[]

export function WorkspaceToolbar({ layout, onPreset, onAdd, onReset }: WorkspaceToolbarProps) {
  const intl = useIntl()
  const t = useCallback((id: string) => intl.formatMessage({ id }), [intl])
  const [addOpen, setAddOpen] = useState(false)

  const addItems: DropdownMenuItem[] = ADDABLE_KINDS.map(kind => ({
    id: `add-${kind}`,
    label: t(`workspace.add.${kind}`),
    icon: kind === 'diff' ? 'difference' : kind === 'preview' ? 'web' : 'terminal',
    disabled: layout.panels.some(p => p.kind === kind),
    onSelect: () => onAdd(kind),
  }))

  return (
    <div
      role="toolbar"
      aria-label={t('workspace.toolbar.aria')}
      data-testid="workspace-toolbar"
      className="flex shrink-0 items-center gap-xs border-b border-outline-variant/10 px-md py-0.5"
    >
      <span className="material-symbols-outlined icon-sm text-on-surface-variant" aria-hidden="true">dashboard</span>
      <div role="group" aria-label={t('workspace.presets.aria')} className="flex items-center gap-0.5">
        {PRESETS.map(({ name, labelId }) => {
          const active = matchesPreset(layout, name)
          return (
            <button
              key={name}
              type="button"
              aria-pressed={active}
              onClick={() => onPreset(name)}
              className={cn(
                'rounded-full px-sm py-0.5 font-label-sm text-label-sm transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
                active
                  ? 'bg-primary/10 text-primary'
                  : 'text-on-surface-variant hover:bg-surface-container hover:text-on-surface',
              )}
            >
              {t(labelId)}
            </button>
          )
        })}
      </div>
      <span className="flex-1" />
      <span className="relative">
        <button
          type="button"
          aria-haspopup="menu"
          aria-expanded={addOpen}
          aria-label={t('workspace.add.aria')}
          title={t('workspace.add.aria')}
          onClick={() => setAddOpen(open => !open)}
          className="p-1 rounded text-on-surface-variant hover:bg-surface-container hover:text-on-surface focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">add_box</span>
        </button>
        <DropdownMenu
          open={addOpen}
          onClose={() => setAddOpen(false)}
          items={addItems}
          ariaLabel={t('workspace.add.aria')}
        />
      </span>
      <button
        type="button"
        onClick={onReset}
        aria-label={t('workspace.reset.aria')}
        title={t('workspace.reset.aria')}
        className="inline-flex items-center gap-0.5 rounded px-xs py-0.5 font-label-sm text-label-sm text-on-surface-variant hover:bg-surface-container hover:text-on-surface focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
      >
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">restart_alt</span>
        {t('workspace.reset')}
      </button>
    </div>
  )
}
