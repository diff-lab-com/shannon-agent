// useTurnUsage — G4b (2026-10-15 parity review §5-G4b 「消息级统计」): the
// per-turn usage feed for MessageBubble's hover detail strip.
//
// DATA-SOURCE RULING (recon 2026-10-15): message-level usage does NOT exist
// on any wire the UI reads. `query:usage` frames are per-LLM-request and
// carry only in/out + a cache HIT-RATE (events.rs UsagePayload — the
// absolute cache_read/cache_creation numbers are dropped before the emit);
// the absolute per-turn tokens (in / out / cache read / cache creation /
// cost) live only on TimelineTurn (crates/shannon-core
// session_log/projections.rs), delivered by the `trace_timeline` IPC. The
// strip therefore renders TURN-granularity figures and says so (「本轮」 in
// its title). This hook reads the SAME projection MessageArea's
// duration/cost lookups use — but as a self-contained module-cached reader,
// because MessageArea (pages/) is outside this batch's file ownership.
//
// Caching contract: one in-flight `trace_timeline` read per session across
// every bubble on screen (the module map dedups the N-bubble mount storm to
// a single IPC). Entries bust when the session's run SETTLES (isQuerying
// true→false) so the just-finished turn appears without a session switch —
// the same A-8 refetch discipline MessageArea applies to its own read.
// Failures degrade to an empty turn list (strip never renders) — usage is
// opportunistic, never a load-bearing surface.

import { createContext, useContext, useEffect, useRef, useState } from 'react'
import * as CatalogModule from '@/context/CatalogContext'
import type { CatalogContextValue } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import type { ModelInfo, ProviderStatus, TimelineTurn } from '@/types'

// Null-safe catalog handle. Several legacy suites mock '@/context/CatalogContext'
// with a bare `{ useCatalog }` factory (no context object on the namespace —
// and vitest's mock namespace THROWS on reading the missing key); a module-level
// fallback keeps the read unbothered — those trees just get a private null
// context (no local hint) instead of crashing. In the real app the real
// context is there and the fallback never applies.
function readCatalogContext(): ReturnType<typeof createContext<CatalogContextValue | null>> {
  try {
    return CatalogModule.CatalogContext ?? createContext<CatalogContextValue | null>(null)
  } catch {
    return createContext<CatalogContextValue | null>(null)
  }
}
const catalogContext = readCatalogContext()

interface TurnUsageCacheEntry {
  /** Shared promise — every subscriber awaits the same IPC. */
  promise: Promise<void>
  /** Resolved turns; null until the promise settles. */
  turns: TimelineTurn[] | null
  /** Session-start model (TurnTimeline.model); null when unset. */
  model: string | null
}

const CACHE_CAP = 8
const cache = new Map<string, TurnUsageCacheEntry>()

function loadTurns(sessionId: string): TurnUsageCacheEntry {
  const hit = cache.get(sessionId)
  if (hit) {
    // LRU refresh: re-insert so the cap evicts the least recently used.
    cache.delete(sessionId)
    cache.set(sessionId, hit)
    return hit
  }
  const entry: TurnUsageCacheEntry = { promise: Promise.resolve(), turns: null, model: null }
  entry.promise = api
    .getTraceTimeline(sessionId)
    .then((tl) => {
      entry.turns = Array.isArray(tl?.turns) ? tl.turns : []
      entry.model = typeof tl?.model === 'string' && tl.model ? tl.model : null
    })
    .catch(() => {
      // Opportunistic surface: a failed read just means no strip.
      entry.turns = []
      entry.model = null
    })
  cache.set(sessionId, entry)
  while (cache.size > CACHE_CAP) {
    const oldest = cache.keys().next().value
    if (oldest === undefined) break
    cache.delete(oldest)
  }
  return entry
}

/** Test seam: drop every cached read so a fixture swap takes effect. */
export function resetTurnUsageCacheForTests(): void {
  cache.clear()
}

/**
 * Local-model signal for the 「本地模型 · $0」 strip segment. Session-grain
 * (the timeline records no per-turn provider): when the session's start
 * model resolves in the catalog, THAT model's provider decides — a session
 * served by a cloud model stays non-local even if the user has since
 * switched the active provider to Ollama. Only when the model is unknown
 * does the ACTIVE provider's wire kind fall through (kind, not id — custom
 * Ollama provider entries can carry any id, but `get_provider_status.kind`
 * is the reliable `ollama` marker).
 */
export function resolveLocalModel(
  timelineModel: string | null | undefined,
  models: ModelInfo[] | null | undefined,
  providerStatus: ProviderStatus | null | undefined,
): boolean {
  if (timelineModel) {
    const known = models?.find(m => m.id === timelineModel || m.name === timelineModel)
    if (known) return known.provider === 'ollama'
  }
  return providerStatus?.kind === 'ollama'
}

export interface TurnUsageInfo {
  /** Turn rows for the session; null until the first read settles (and []
   *  when the read failed or the log has no turns). */
  turns: TimelineTurn[] | null
  /** Session-start model id, for local-provider resolution. */
  model: string | null
  /** True when the session's serving model/provider is local (ollama). */
  isLocal: boolean
}

const IDLE: TurnUsageInfo = { turns: null, model: null, isLocal: false }

/**
 * Subscribe one bubble to the session's turn-usage projection. `isQuerying`
 * is the visible session's run state — its true→false settle edge busts the
 * cache so the finished turn's authoritative numbers arrive exactly once.
 */
export function useTurnUsage(sessionId: string | null, isQuerying: boolean): TurnUsageInfo {
  // Null-safe catalog read (useContext, NOT the throwing useCatalog()): the
  // strip must render — minus the local hint — in isolated trees that carry
  // no CatalogProvider (existing bubble tests, previews).
  const catalog = useContext(catalogContext)
  const models = catalog?.models
  const providerStatus = catalog?.providerStatus

  // One tick per settle transition (not per render) busts the session's
  // cache entry; every bubble observes the same edge and shares the refetch.
  const [settleTick, setSettleTick] = useState(0)
  const wasQueryingRef = useRef(isQuerying)
  useEffect(() => {
    if (wasQueryingRef.current && !isQuerying && sessionId) cache.delete(sessionId)
    if (wasQueryingRef.current && !isQuerying) setSettleTick(t => t + 1)
    wasQueryingRef.current = isQuerying
  }, [isQuerying, sessionId])

  const [info, setInfo] = useState<TurnUsageInfo>(IDLE)
  useEffect(() => {
    if (!sessionId) {
      setInfo(IDLE)
      return
    }
    let cancelled = false
    const entry = loadTurns(sessionId)
    void entry.promise.then(() => {
      if (cancelled) return
      setInfo({
        turns: entry.turns,
        model: entry.model,
        isLocal: resolveLocalModel(entry.model, models, providerStatus),
      })
    })
    return () => { cancelled = true }
  }, [sessionId, settleTick, models, providerStatus])
  return info
}
