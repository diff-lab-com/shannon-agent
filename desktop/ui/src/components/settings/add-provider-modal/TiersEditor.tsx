// Add/Edit Provider modal — advanced disclosure section: per-tier model
// overrides (fast/standard/pro). Extracted from AddProviderModal.tsx (T3.1).
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface TiersEditorProps {
  tiers: { fast: string; standard: string; pro: string }
  /// The provider's currently-active model id (from `editing.model`).
  /// Used to badge the row whose override matches the running model so the
  /// user can see at a glance which tier their engine is actually using.
  activeModelId: string
  onChange: (t: { fast: string; standard: string; pro: string }) => void
}

export function TiersEditor({ tiers, activeModelId, onChange }: TiersEditorProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const rows: Array<{ key: 'fast' | 'standard' | 'pro'; labelKey: string }> = [
    { key: 'fast', labelKey: 'settings.models.addProvider.tierFast' },
    { key: 'standard', labelKey: 'settings.models.addProvider.tierStandard' },
    { key: 'pro', labelKey: 'settings.models.addProvider.tierPro' },
  ]
  return (
    <div>
      <p className="font-label-sm text-on-surface-variant mb-xs">{t('settings.models.providers.tiers')}</p>
      <p className="font-label-xs text-on-surface-variant opacity-70 mb-xs">
        {t('settings.models.addProvider.tiersHelp')}
      </p>
      <div className="space-y-xs">
        {rows.map(({ key, labelKey }) => {
          const isActive = tiers[key].trim() === activeModelId.trim() && activeModelId.trim().length > 0
          return (
            <label key={key} className="flex items-center gap-sm" data-testid={`tier-${key}-row`}>
              <span className="font-label-sm text-on-surface-variant w-20 flex items-center gap-xs">
                {t(labelKey)}
                {isActive ? (
                  <span
                    className="inline-flex items-center px-xs py-0.5 rounded-full bg-primary/10 text-primary font-label-xs"
                    data-testid={`tier-${key}-active`}
                    title={t('settings.models.addProvider.tierActiveBadge')}
                  >
                    {t('settings.models.addProvider.tierActiveBadge')}
                  </span>
                ) : null}
              </span>
              <Input
                className="flex-1 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded font-body-xs font-mono"
                value={tiers[key]}
                placeholder="model-id"
                onChange={(e) => onChange({ ...tiers, [key]: e.target.value })}
                data-testid={`tier-${key}-input`}
              />
              {tiers[key] ? (
                <Button
                  variant="ghost"
                  type="button"
                  className="px-sm py-xs text-on-surface-variant hover:text-error cursor-pointer"
                  onClick={() => onChange({ ...tiers, [key]: '' })}
                  aria-label={t('settings.models.addProvider.tierClear')}
                  data-testid={`tier-${key}-clear`}
                >
                  <span className="material-symbols-outlined text-[16px]">close</span>
                </Button>
              ) : null}
            </label>
          )
        })}
      </div>
    </div>
  )
}
