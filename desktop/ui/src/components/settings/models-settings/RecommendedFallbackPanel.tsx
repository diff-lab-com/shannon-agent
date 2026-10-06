// S3-4 (推荐降级链) — the inline "recommended fallback chain" panel under a
// provider card (and reachable from the Test-all results' failed rows).
//
// Contract: the recommendation is CANDIDATES ONLY — the backend computes it
// from the profile roster + tier catalog without persisting anything, and
// this panel is the explicit-confirmation gate. Nothing reaches the slot's
// `fallback_models` until the user presses "Save this chain" (the behavioral
// red line: failover is never enabled automatically). The Apply button is
// the only writer; it goes through `setProviderFallbackModels` (surgical
// field write — the active provider/model pointer cannot move).
//
// Per-hop semantics mirror the engine (docs/configuration.md, failover
// section): hops apply in order when a request keeps failing with 429/5xx
// BEFORE the stream starts; a bare entry stays on the provider (model swap),
// a qualified `provider/model` entry switches provider within the roster.
// Mid-stream drops reconnect on the same provider and never fail over, and
// a session pinned to a model never fails over (same wording as the keys
// panel's pinNote).
import { useEffect, useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import { toast } from 'sonner'
import { Spinner } from '@/components/ui/loading-state'
import { Button } from '@/components/ui/button'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import type { ProviderConnection } from '@/types'
import { cn } from '@/lib/utils'

interface RecommendedFallbackPanelProps {
  conn: ProviderConnection
  /** Close the panel (clear the target in the parent). */
  onClose: () => void
  /** A chain was committed backend-side — the parent refreshes the roster
   *  (the connection's `fallback_models` changed). */
  onApplied: () => Promise<void> | void
}

/** Canonical tier → the same label keys the tier rows in the provider
 *  modal/advanced section use, so the wording stays identical. */
const TIER_LABEL_KEY: Record<string, string> = {
  pro: 'settings.models.providers.tierPro',
  standard: 'settings.models.providers.tierStandard',
  fast: 'settings.models.providers.tierFast',
}

export function RecommendedFallbackPanel({
  conn,
  onClose,
  onApplied,
}: RecommendedFallbackPanelProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) =>
    intl.formatMessage({ id }, values)
  const [chain, setChain] = useState<api.RecommendedFallbackChain | null>(null)
  const [loading, setLoading] = useState(true)
  const [loadFailed, setLoadFailed] = useState(false)
  const [applying, setApplying] = useState(false)

  useEffect(() => {
    let cancelled = false
    api
      .recommendFallbackChain(conn.id)
      .then((c) => {
        if (!cancelled) setChain(c)
      })
      .catch((e) => {
        if (!cancelled) {
          setLoadFailed(true)
          console.warn('recommendFallbackChain error:', e)
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [conn.id])

  const apply = async () => {
    if (!chain || chain.hops.length === 0 || applying) return
    setApplying(true)
    try {
      await api.setProviderFallbackModels(
        conn.id,
        chain.hops.map((h) => h.entry),
      )
      toast.success(t('settings.models.providers.fallbackApplied', { label: conn.display_name }))
      await onApplied()
    } catch (e) {
      toastError(t('settings.models.providers.fallbackApplyFailed'), e)
    } finally {
      setApplying(false)
    }
  }

  return (
    <section
      className="p-md rounded-xl border border-primary/40 bg-surface-container-low/60"
      aria-label={t('settings.models.providers.fallbackTitle')}
      data-testid="recommend-fallback-panel"
    >
      <div className="flex items-start justify-between gap-md mb-sm">
        <div>
          <h4 className="font-headline-sm text-on-surface flex items-center gap-xs">
            <span className="material-symbols-outlined icon-md text-primary" aria-hidden="true">
              alt_route
            </span>
            {t('settings.models.providers.fallbackTitle')}
            <span className="font-label-md text-on-surface-variant">· {conn.display_name}</span>
          </h4>
        </div>
        <Button
          variant="ghost"
          onClick={onClose}
          aria-label={t('settings.models.providers.cancel')}
          data-testid="recommend-fallback-close"
          className="h-auto py-xs px-sm rounded-lg text-on-surface-variant hover:text-primary cursor-pointer shrink-0"
        >
          <span className="material-symbols-outlined icon-md" aria-hidden="true">
            close
          </span>
        </Button>
      </div>

      {/* Trigger + scope semantics — the degradation contract in two lines
          (P-N13 documentation mirror; same pin wording as the keys panel). */}
      <p className="text-label-sm text-on-surface-variant flex items-start gap-xs mb-xs">
        <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">
          bolt
        </span>
        <span>{t('settings.models.providers.fallbackHelp')}</span>
      </p>
      <p className="text-label-sm text-on-surface-variant flex items-start gap-xs mb-md">
        <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">
          push_pin
        </span>
        <span>{t('settings.models.providers.fallbackScopeNote')}</span>
      </p>

      {loading ? (
        <p className="text-body-sm text-on-surface-variant py-sm flex items-center gap-sm">
          <Spinner className="text-primary icon-sm" />
          {t('settings.models.providers.fallbackComputing')}
        </p>
      ) : loadFailed ? (
        <p role="alert" className="text-body-sm text-error py-sm" data-testid="recommend-fallback-failed">
          {t('settings.models.providers.fallbackLoadFailed')}
        </p>
      ) : !chain || chain.hops.length === 0 ? (
        <p
          className="text-body-sm text-on-surface-variant py-sm"
          data-testid="recommend-fallback-empty"
        >
          {t('settings.models.providers.fallbackEmpty')}
        </p>
      ) : (
        <>
          <ol className="space-y-xs mb-md" data-testid="recommend-fallback-hops">
            {chain.hops.map((hop, i) => (
              <li
                key={hop.entry}
                className="flex items-center justify-between gap-md px-md py-sm rounded-lg border border-outline-variant/40"
                data-testid="recommend-fallback-hop"
                data-hop-entry={hop.entry}
              >
                <div className="flex items-center gap-sm min-w-0">
                  <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">
                    {i === 0 ? 'looks_one' : i === 1 ? 'looks_two' : 'looks_3'}
                  </span>
                  <span className="font-label-sm font-mono text-on-surface truncate" title={hop.entry}>
                    {hop.model}
                  </span>
                  <span className="px-xs py-[2px] bg-surface-container-high text-on-surface-variant rounded-sm text-label-2xs font-bold uppercase tracking-wider shrink-0">
                    {t(TIER_LABEL_KEY[hop.tier] ?? 'settings.models.providers.tierStandard')}
                  </span>
                </div>
                <span
                  className={cn(
                    'font-label-xs text-label-xs shrink-0',
                    hop.same_provider ? 'text-on-surface-variant' : 'text-primary',
                  )}
                >
                  {hop.same_provider
                    ? t('settings.models.providers.fallbackSameProvider', {
                        provider: hop.provider_label,
                      })
                    : t('settings.models.providers.fallbackSwitchProvider', {
                        provider: hop.provider_label,
                      })}
                </span>
              </li>
            ))}
          </ol>
          <div className="flex items-center justify-end gap-sm">
            <Button
              variant="ghost"
              onClick={onClose}
              disabled={applying}
              className="h-auto py-xs px-md rounded-lg font-label-sm text-on-surface-variant hover:text-primary cursor-pointer"
            >
              {t('settings.models.providers.cancel')}
            </Button>
            <Button
              onClick={() => {
                void apply()
              }}
              disabled={applying}
              data-testid="recommend-fallback-apply"
              className="h-auto py-xs px-md rounded-lg font-label-sm bg-primary text-on-primary hover:bg-primary/90 cursor-pointer disabled:opacity-50 flex items-center gap-xs"
            >
              {applying ? (
                <>
                  <Spinner className="icon-sm" />
                  {t('settings.models.providers.saving')}
                </>
              ) : (
                t('settings.models.providers.fallbackApply')
              )}
            </Button>
          </div>
        </>
      )}
    </section>
  )
}
