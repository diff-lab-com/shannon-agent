// Add/Edit Provider modal — advanced disclosure section: extra HTTP headers
// editor (key/value rows with add/remove). Extracted from AddProviderModal.tsx
// (T3.1).
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import type { HeaderRow } from './types'

interface HeaderRowsEditorProps {
  rows: HeaderRow[]
  onChange: (rows: HeaderRow[]) => void
}

export function HeaderRowsEditor({ rows, onChange }: HeaderRowsEditorProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return (
    <div>
      <p className="font-label-sm text-on-surface-variant mb-xs">{t('settings.models.providers.extraHeaders')}</p>
      {rows.length === 0 ? (
        <p className="font-label-xs text-on-surface-variant opacity-60 mb-xs" data-testid="extra-headers-empty">
          {t('settings.models.providers.extraHeadersEmpty')}
        </p>
      ) : null}
      <div className="space-y-xs">
        {rows.map((row, i) => (
          <div key={i} className="flex items-center gap-xs" data-testid="extra-headers-row">
            <Input
              className="flex-1 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded font-body-xs font-mono"
              value={row.key}
              placeholder={t('settings.models.providers.extraHeadersKey')}
              onChange={(e) => {
                const next = rows.slice()
                next[i] = { ...row, key: e.target.value }
                onChange(next)
              }}
            />
            <Input
              className="flex-1 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded font-body-xs font-mono"
              value={row.value}
              placeholder={t('settings.models.providers.extraHeadersValue')}
              onChange={(e) => {
                const next = rows.slice()
                next[i] = { ...row, value: e.target.value }
                onChange(next)
              }}
            />
            <Button
              variant="ghost"
              type="button"
              className="px-sm py-xs text-on-surface-variant hover:text-error cursor-pointer"
              onClick={() => onChange(rows.filter((_, j) => j !== i))}
              aria-label={t('settings.models.providers.extraHeadersRemove')}
            >
              <span className="material-symbols-outlined text-[16px]">close</span>
            </Button>
          </div>
        ))}
      </div>
      <Button
        variant="ghost"
        type="button"
        className="mt-xs px-sm py-xs font-label-sm text-primary hover:bg-primary/10 cursor-pointer"
        onClick={() => onChange([...rows, { key: '', value: '' }])}
        data-testid="extra-headers-add"
      >
        <span className="material-symbols-outlined text-[16px] mr-xs">add</span>
        {t('settings.models.providers.extraHeadersAdd')}
      </Button>
    </div>
  )
}
