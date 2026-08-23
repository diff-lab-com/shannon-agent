import type { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import type { ProviderConnection } from '@/types'
import { KIND_INFO, kindLabel } from './types'

export function ProviderCard({
  conn,
  isActive,
  testingId,
  activatingId,
  intl,
  t,
  onTest,
  onActivate,
  onEdit,
  onDelete,
}: {
  conn: ProviderConnection
  isActive: boolean
  testingId: string | null
  activatingId: string | null
  intl: ReturnType<typeof useIntl>
  t: (id: string) => string
  onTest: () => void
  onActivate: () => void
  onEdit: () => void
  onDelete: () => void
}) {
  const hasKey = conn.has_api_key
  const info = KIND_INFO[conn.kind]
  return (
    <div
      className={`p-md rounded-xl border flex items-center justify-between transition-colors ${
        isActive ? 'border-2 border-primary bg-primary-container/5' : 'border-outline-variant/50'
      }`}
    >
      <div className="flex items-center gap-md min-w-0">
        <div className={`w-9 h-9 rounded-lg flex items-center justify-center shrink-0 ${isActive ? 'bg-primary text-on-primary' : 'bg-surface-container-high text-on-surface-variant'}`}>
          <span className="material-symbols-outlined icon-md">{info?.icon ?? 'hub'}</span>
        </div>
        <div className="min-w-0">
          <div className="flex items-center gap-xs">
            <span className="font-label-md font-bold text-on-surface truncate">{conn.display_name}</span>
            {isActive ? (
              <span className="px-xs py-[1px] bg-primary text-on-primary rounded text-[10px] font-bold shrink-0">{t('settings.models.providers.activeBadge')}</span>
            ) : null}
          </div>
          <div className="flex items-center gap-xs flex-wrap">
            <span className="font-label-xs text-[11px] text-on-surface-variant">{kindLabel(intl, conn.kind)}</span>
            {conn.base_url ? (
              <span className="font-label-xs text-[11px] text-on-surface-variant font-mono truncate max-w-[260px]" title={conn.base_url ?? undefined}>{conn.base_url}</span>
            ) : null}
            <span
              className={`inline-flex items-center gap-[2px] font-label-xs text-[10px] ${hasKey ? 'text-primary' : 'text-on-surface-variant opacity-60'}`}
              title={hasKey ? t('settings.models.providers.keySet') : t('settings.models.providers.keyMissing')}
            >
              <span className="material-symbols-outlined text-[12px]">{hasKey ? 'key' : 'key_off'}</span>
              {hasKey ? t('settings.models.providers.keySet') : t('settings.models.providers.keyMissing')}
            </span>
          </div>
        </div>
      </div>
      <div className="flex items-center gap-xs shrink-0">
        {testingId === conn.id ? (
          <span className="material-symbols-outlined text-primary animate-spin text-[18px]">progress_activity</span>
        ) : (
          <Button variant="ghost" className="px-sm py-xs text-on-surface-variant hover:text-primary cursor-pointer" onClick={onTest} aria-label={t('settings.models.providers.test')}>
            <span className="material-symbols-outlined text-[18px]">cable</span>
          </Button>
        )}
        {!isActive ? (
          <Button
            variant="ghost"
            className="px-sm py-xs text-primary hover:bg-primary/10 cursor-pointer disabled:opacity-50"
            onClick={onActivate}
            disabled={activatingId !== null}
          >
            {activatingId === conn.id ? (
              <span className="material-symbols-outlined text-[16px] animate-spin align-middle">progress_activity</span>
            ) : t('settings.models.providers.activate')}
          </Button>
        ) : null}
        <Button variant="ghost" className="px-sm py-xs text-on-surface-variant hover:text-primary cursor-pointer" onClick={onEdit} aria-label={t('settings.models.providers.edit')}>
          <span className="material-symbols-outlined text-[18px]">edit</span>
        </Button>
        <Button variant="ghost" className="px-sm py-xs text-on-surface-variant hover:text-error cursor-pointer" onClick={onDelete} aria-label={t('settings.models.providers.delete')}>
          <span className="material-symbols-outlined text-[18px]">delete</span>
        </Button>
      </div>
    </div>
  )
}