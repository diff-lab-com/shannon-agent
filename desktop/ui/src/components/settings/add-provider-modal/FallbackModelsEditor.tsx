// Add/Edit Provider modal — advanced disclosure section: fallback model
// list editor. Extracted from AddProviderModal.tsx (T3.1).
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface FallbackModelsEditorProps {
  models: string[]
  onChange: (models: string[]) => void
}

export function FallbackModelsEditor({ models, onChange }: FallbackModelsEditorProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return (
    <div>
      <p className="font-label-sm text-on-surface-variant mb-xs">{t('settings.models.addProvider.fallbackModels')}</p>
      <p className="font-label-xs text-on-surface-variant opacity-70 mb-xs">
        {t('settings.models.addProvider.fallbackModelsHelp')}
      </p>
      {models.length === 0 ? (
        <p className="font-label-xs text-on-surface-variant opacity-60 mb-xs" data-testid="fallback-models-empty">
          {t('settings.models.addProvider.fallbackModelsEmpty')}
        </p>
      ) : null}
      <div className="space-y-xs">
        {models.map((m, i) => (
          <div key={i} className="flex items-center gap-xs" data-testid="fallback-models-row">
            <Input
              className="flex-1 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded font-body-xs font-mono"
              value={m}
              placeholder={t('settings.models.addProvider.fallbackModelsPlaceholder')}
              onChange={(e) => {
                const next = models.slice()
                next[i] = e.target.value
                onChange(next)
              }}
            />
            <Button
              variant="ghost"
              type="button"
              className="px-sm py-xs text-on-surface-variant hover:text-error cursor-pointer"
              onClick={() => onChange(models.filter((_, j) => j !== i))}
              aria-label={t('settings.models.addProvider.fallbackModelsRemove')}
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
        onClick={() => onChange([...models, ''])}
        data-testid="fallback-models-add"
      >
        <span className="material-symbols-outlined text-[16px] mr-xs">add</span>
        {t('settings.models.addProvider.fallbackModelsAdd')}
      </Button>
    </div>
  )
}
