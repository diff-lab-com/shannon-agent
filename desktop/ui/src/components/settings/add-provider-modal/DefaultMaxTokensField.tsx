// Add/Edit Provider modal — advanced disclosure section: numeric
// defaultMaxTokens override. Extracted from AddProviderModal.tsx (T3.1).
import { useIntl } from 'react-intl'
import { Input } from '@/components/ui/input'

interface DefaultMaxTokensFieldProps {
  value: string
  onChange: (v: string) => void
}

export function DefaultMaxTokensField({ value, onChange }: DefaultMaxTokensFieldProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return (
    <div>
      <p className="font-label-sm text-on-surface-variant mb-xs">{t('settings.models.providers.defaultMaxTokens')}</p>
      <Input
        className="w-40 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded font-body-sm font-mono"
        type="number"
        min={1}
        max={200000}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        data-testid="default-max-tokens"
      />
    </div>
  )
}
