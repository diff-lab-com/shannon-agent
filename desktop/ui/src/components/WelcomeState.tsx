import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { TextLoop } from '@/components/reactbits/TextLoop'
import { useCatalog } from '@/context/CatalogContext'
import { formatShortcut } from '@/lib/platform'
import * as api from '@/lib/tauri-api'
import { WELCOME_EXAMPLES, shuffleExamples } from './welcomeExamples'

interface WelcomeStateProps {
  onSelectPrompt: (prompt: string) => void
}

export default function WelcomeState({ onSelectPrompt }: WelcomeStateProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const navigate = useNavigate()
  const { providerStatus, config } = useCatalog()
  // Cycled subtitle items. Order mirrors the example-card order below so the
  // highlighted verb ("draft emails") cues the next card the user is likely to
  // reach for. TextLoop honors prefers-reduced-motion (static) and window blur
  // (paused) — see T2.1 guards.
  const loopItems = [
    t('welcomeState.subtitleItem.email'),
    t('welcomeState.subtitleItem.summarize'),
    t('welcomeState.subtitleItem.research'),
    t('welcomeState.subtitleItem.code'),
  ]
  // D5 方案① (主动任务推荐) — the presentation gate + the two gated
  // affordances on this card: the 「换一批」 refresh and workspace-aware
  // example filtering. Gate default ON (missing key / config not loaded yet
  // → shown); a probe failure is silent and fail-open (all cards render).
  const suggestionsEnabled = config?.suggestions_enabled ?? true
  // Workspace code markers from `detect_workspace_markers`. null = probe
  // pending / failed / gate off → NO filtering (render exactly as before).
  const [markers, setMarkers] = useState<string[] | null>(null)
  // 「换一批」 deck: a shuffled copy of the static examples. The base array
  // is never mutated, so a gate-off card (or the sidebar's first-two slice)
  // keeps the original order.
  const [deck, setDeck] = useState(WELCOME_EXAMPLES)
  useEffect(() => {
    if (!suggestionsEnabled) return
    let cancelled = false
    api.detectWorkspaceMarkers()
      .then(found => { if (!cancelled) setMarkers(found) })
      .catch(() => { /* probe failed → fail open, stay silent */ })
    return () => { cancelled = true }
  }, [suggestionsEnabled])
  // Coding-oriented cards (research/code) need a marker; office cards
  // (email/summarize) always show. No filtering until the probe has spoken
  // (markers null = pending/failed → today's render, fail open), and the
  // gate-off render never consults the probe at all — byte-identical to
  // the pre-D5 card.
  const visibleExamples = suggestionsEnabled && markers != null
    ? deck.filter(ex => !ex.coding || markers.length > 0)
    : deck
  // Localized prompt per example (review §5: prompts used to be en-only).
  const promptOf = (ex: (typeof WELCOME_EXAMPLES)[number]) =>
    intl.formatMessage({ id: ex.promptKey })
  // Review §3-A1 (item e): the empty chat canvas is where unconfigured users
  // land after skipping Welcome — surface the provider CTA here, but only on
  // a POSITIVE unconfigured signal (no active provider AND no env fallback).
  // `null` (snapshot still loading / read failed) and any configured shape
  // never show it.
  const unconfigured = !!providerStatus
    && providerStatus.active_provider_id == null
    && providerStatus.env_provider == null
  return (
    <div className="flex items-center justify-center h-full min-h-full">
      <div className="text-center max-w-[560px] w-full mx-auto px-lg">
        <div className="w-9 h-9 rounded-full bg-primary-container/40 flex items-center justify-center mx-auto mb-md">
          <span className="material-symbols-outlined icon-md text-primary">auto_awesome</span>
        </div>
        <h2 className="font-headline-md text-headline-md text-on-surface mb-xs">{t('welcomeState.title')}</h2>
        <p className="font-body-md text-on-surface-variant mb-xl">
          {t('welcomeState.subtitlePrefix')}{' '}
          <TextLoop items={loopItems} className="text-primary font-medium" />
        </p>
        {unconfigured && (
          <div
            data-testid="welcome-provider-cta"
            className="mb-lg rounded-xl border border-primary/30 bg-primary/5 p-md flex flex-col items-center gap-sm"
          >
            <span className="material-symbols-outlined icon-md text-primary">key_alert</span>
            <p className="font-label-lg text-on-surface font-bold">{t('welcomeState.providerCta.title')}</p>
            <p className="font-body-sm text-on-surface-variant">{t('welcomeState.providerCta.body')}</p>
            <Button
              type="button"
              onClick={() => navigate('/settings/models')}
              className="px-lg py-sm rounded-lg bg-primary text-on-primary font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
            >
              {t('welcomeState.providerCta.cta')}
            </Button>
          </div>
        )}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-sm">
          {visibleExamples.map(ex => (
            <Button
              key={ex.icon}
              variant="outline"
              className="h-auto justify-start items-start text-left whitespace-normal p-md rounded-xl hover:bg-surface-container-high hover:border-primary/30 cursor-pointer group"
              onClick={() => onSelectPrompt(promptOf(ex))}
            >
              <span className="material-symbols-outlined icon-md text-on-surface-variant mt-0.5 group-hover:text-primary transition-colors">{ex.icon}</span>
              <div className="min-w-0">
                <p className="font-label-md text-on-surface font-bold">{t(ex.titleKey)}</p>
                <p className="font-body-sm text-on-surface-variant line-clamp-2">{promptOf(ex)}</p>
              </div>
            </Button>
          ))}
        </div>
        {/* D5 方案① 「换一批」: local reshuffle of the static deck — no
            backend round trip, no model calls. Only on the gated card; the
            gate-off render is byte-identical to the pre-D5 layout. */}
        {suggestionsEnabled && (
          <div className="mt-md flex justify-center">
            <Button
              type="button"
              variant="ghost"
              data-testid="welcome-refresh"
              aria-label={t('welcomeState.refresh')}
              onClick={() => setDeck(shuffleExamples(WELCOME_EXAMPLES))}
              className="mt-sm px-md py-xs rounded-lg font-label-sm text-on-surface-variant hover:text-primary hover:bg-surface-container-high cursor-pointer"
            >
              <span className="material-symbols-outlined icon-md" aria-hidden="true">shuffle</span>
              {t('welcomeState.refresh')}
            </Button>
          </div>
        )}
        <div className="mt-xl flex items-center justify-center gap-lg text-on-surface-variant opacity-50">
          <span className="flex items-center gap-xs text-label-sm"><kbd className="px-1.5 py-0.5 rounded-sm bg-surface-container-high text-on-surface-variant font-mono text-label-xs">{formatShortcut('K')}</kbd> {t('welcomeState.shortcuts.commands')}</span>
          <span className="flex items-center gap-xs text-label-sm"><kbd className="px-1.5 py-0.5 rounded-sm bg-surface-container-high text-on-surface-variant font-mono text-label-xs">?</kbd> {t('welcomeState.shortcuts.shortcuts')}</span>
          <span className="flex items-center gap-xs text-label-sm"><kbd className="px-1.5 py-0.5 rounded-sm bg-surface-container-high text-on-surface-variant font-mono text-label-xs">Alt+Up</kbd> {t('welcomeState.shortcuts.history')}</span>
        </div>
      </div>
    </div>
  )
}
