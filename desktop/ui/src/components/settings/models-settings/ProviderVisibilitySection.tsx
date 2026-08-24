import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { PROVIDER_KINDS_FOR_VISIBILITY, kindLabel } from './types'
import { cn } from '@/lib/utils'

export function ProviderVisibilitySection({
  onChanged,
}: {
  onChanged: () => Promise<void>
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  // `null` ⇒ no desktop override (engine env vars decide).
  // `Some([])` ⇒ user toggled every provider off.
  // `Some([..])` ⇒ explicit allowlist.
  const [override, setOverride] = useState<string[] | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    let cancelled = false
    api.getProviderAllowlist()
      .then(() => {
        if (!cancelled) {
          // The backend returns the *effective* allowlist (env vars
          // resolved when desktop override is `null`). For the UI we
          // want the desktop override specifically — `null` ⇒
          // "use env vars", `Some([])` ⇒ "all off",
          // `Some(non_empty)` ⇒ explicit list. Read `enabled_providers`
          // through `getConfig` to disambiguate.
          api.getConfig().then((cfg) => {
            if (cancelled) return
            const ep = (cfg as { enabled_providers?: string[] | null }).enabled_providers
            setOverride(ep === undefined ? null : ep)
            setLoading(false)
          }).catch(() => { if (!cancelled) setLoading(false) })
        }
      })
      .catch(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [])

  const isChecked = (kind: string): boolean => {
    // `null` (no override) ⇒ all checked (engine env-var decides).
    // `Some([])` ⇒ none checked. `Some(non_empty)` ⇒ only those.
    if (override === null) return true
    return override.includes(kind)
  }

  const toggle = async (kind: string) => {
    if (saving) return
    const current = override === null
      ? [...PROVIDER_KINDS_FOR_VISIBILITY]
      : override
    const next = current.includes(kind)
      ? current.filter((k) => k !== kind)
      : [...current, kind]
    setSaving(true)
    try {
      await api.configure({
        key: 'enabled_providers',
        value: JSON.stringify(next),
      })
      setOverride(next)
      await onChanged()
    } catch (e) {
      toastError(t('settings.models.providers.saveFailed'), e)
    } finally {
      setSaving(false)
    }
  }

  const resetToDefault = async () => {
    if (saving) return
    setSaving(true)
    try {
      // `null` clears the desktop override (falls back to env vars).
      await api.configure({ key: 'enabled_providers', value: 'null' })
      setOverride(null)
      await onChanged()
      toast.success(t('settings.models.providerVisibility.reset'))
    } catch (e) {
      toastError(t('settings.models.providers.saveFailed'), e)
    } finally {
      setSaving(false)
    }
  }

  return (
    <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-sm">
      <div className="flex items-start justify-between mb-md gap-md">
        <div>
          <h3 className="font-headline-md text-on-surface">
            {t('settings.models.providerVisibility.title')}
          </h3>
          <p className="text-body-sm text-on-surface-variant mt-xs">
            {t('settings.models.providerVisibility.subtitle')}
          </p>
        </div>
        <Button
          variant="ghost"
          className="px-md py-sm text-on-surface-variant hover:text-primary whitespace-nowrap cursor-pointer disabled:opacity-50"
          onClick={resetToDefault}
          disabled={loading || saving || override === null}
        >
          {t('settings.models.providerVisibility.reset')}
        </Button>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-sm">
        {PROVIDER_KINDS_FOR_VISIBILITY.map((kind) => {
          const checked = isChecked(kind)
          return (
            <label
              key={kind}
              className={cn(
                "flex items-center gap-md p-sm rounded-lg border cursor-pointer transition-colors",
                checked
                  ? 'border-primary/50 bg-primary-container/5'
                  : 'border-outline-variant/30 hover:border-outline-variant',
              )}
            >
              <input
                type="checkbox"
                className="w-4 h-4 cursor-pointer accent-primary"
                checked={checked}
                disabled={loading || saving}
                onChange={() => toggle(kind)}
              />
              <span className="font-label-md text-on-surface">
                {kindLabel(intl, kind)}
              </span>
            </label>
          )
        })}
      </div>
    </section>
  )
}