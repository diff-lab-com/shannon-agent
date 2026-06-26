import { useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import type { ScheduledRoutine } from '@/types'

interface Props {
  onInstantiated: (routine: ScheduledRoutine) => void
}

const CATEGORY_ICONS: Record<string, string> = {
  engineering: 'code',
  security: 'shield',
  productivity: 'task_alt',
  finops: 'payments',
  documentation: 'description',
  operations: 'dns',
}

export default function RoutineTemplatesBrowser({ onInstantiated }: Props) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [templates, setTemplates] = useState<api.RoutineTemplate[]>([])
  const [loading, setLoading] = useState(true)
  const [filter, setFilter] = useState<string>('all')
  const [instantiating, setInstantiating] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    api
      .listRoutineTemplates()
      .then((list) => {
        if (!cancelled) setTemplates(list)
      })
      .catch((e) => {
        console.warn('listRoutineTemplates error:', e)
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const categories = useMemo(() => {
    const set = new Set(templates.map((t) => t.category))
    return ['all', ...Array.from(set).sort()]
  }, [templates])

  const visible = useMemo(() => {
    if (filter === 'all') return templates
    return templates.filter((t) => t.category === filter)
  }, [templates, filter])

  const handleInstantiate = async (template: api.RoutineTemplate) => {
    setInstantiating(template.id)
    try {
      const created = await api.instantiateRoutineTemplate(template.id)
      toast.success(t('routines.templates.instantiated'), { description: created.name })
      onInstantiated(created)
    } catch (e) {
      console.warn('instantiateRoutineTemplate error:', e)
      toast.error(
        typeof e === 'string' ? e : t('routines.templates.error.instantiateFailed'),
      )
    }
    setInstantiating(null)
  }

  if (loading) {
    return (
      <div
        className="flex items-center justify-center py-lg"
        role="status"
        aria-live="polite"
      >
        <span
          className="material-symbols-outlined text-[28px] text-primary animate-spin"
          aria-hidden="true"
        >
          progress_activity
        </span>
        <span className="sr-only">{t('routines.templates.loading')}</span>
      </div>
    )
  }

  if (templates.length === 0) {
    return (
      <p className="text-on-surface-variant font-body-sm italic">
        {t('routines.templates.empty')}
      </p>
    )
  }

  return (
    <div className="space-y-md">
      <div className="flex items-center justify-between gap-sm">
        <h2 className="font-headline-md text-on-surface">
          {t('routines.templates.title')}
        </h2>
        {categories.length > 2 && (
          <select
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            aria-label={t('routines.templates.filter.ariaLabel')}
            className="px-md py-xs rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
          >
            {categories.map((c) => (
              <option key={c} value={c}>
                {c === 'all'
                  ? t('routines.templates.filter.all')
                  : t(`routines.templates.category.${c}`) !== `routines.templates.category.${c}`
                    ? t(`routines.templates.category.${c}`)
                    : c}
              </option>
            ))}
          </select>
        )}
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
        {visible.map((tmpl) => (
          <div
            key={tmpl.id}
            className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-md flex flex-col gap-sm"
          >
            <div className="flex items-start gap-sm">
              <span
                className="material-symbols-outlined text-primary shrink-0"
                aria-hidden="true"
              >
                {CATEGORY_ICONS[tmpl.category] ?? 'bolt'}
              </span>
              <div className="flex-1 min-w-0">
                <h3 className="font-label-md text-on-surface truncate">{tmpl.name}</h3>
                <p className="font-label-sm text-on-surface-variant uppercase tracking-wide">
                  {tmpl.category}
                </p>
              </div>
            </div>
            <p className="font-body-sm text-on-surface-variant flex-1">
              {tmpl.description}
            </p>
            <div className="flex items-center justify-between gap-sm pt-xs">
              <code className="font-mono text-[11px] text-on-surface-variant bg-surface-container-high px-xs py-xxs rounded">
                {tmpl.trigger_type === 'cron'
                  ? tmpl.cron_expr ?? ''
                  : `${tmpl.interval_secs ?? 0}s`}
              </code>
              <button
                onClick={() => handleInstantiate(tmpl)}
                disabled={instantiating !== null}
                className="px-md py-xs bg-primary text-on-primary rounded-md font-label-sm cursor-pointer hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {instantiating === tmpl.id
                  ? t('routines.templates.installing')
                  : t('routines.templates.use')}
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
