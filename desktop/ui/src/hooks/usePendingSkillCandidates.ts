import { useCallback, useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { listSkillCandidates, type SkillCandidate } from '@/lib/tauri-api'

// Candidates only change when the pattern detector runs or the user (or an
// automation) approves/rejects one — the backend emits
// `skill-candidates-changed` on each. The interval remains only as a slow
// safety net (and as the refresh path in demo mode, where events don't
// reach a plain browser).
const FALLBACK_POLL_MS = 5 * 60_000

export function usePendingSkillCandidates(): {
  candidates: SkillCandidate[]
  loading: boolean
  /** B3 P1-17: last load failure, so the UI can tell "nothing pending"
   *  apart from "the queue could not be read". Null on success. */
  error: string | null
  refetch: () => void
} {
  const [candidates, setCandidates] = useState<SkillCandidate[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const cancelledRef = useRef(false)

  const refetch = useCallback(() => {
    listSkillCandidates()
      .then((rows) => {
        if (!cancelledRef.current) {
          setCandidates(Array.isArray(rows) ? rows : [])
          setError(null)
        }
      })
      .catch((err) => {
        if (!cancelledRef.current) {
          setCandidates([])
          setError(err instanceof Error ? err.message : String(err))
        }
      })
      .finally(() => {
        if (!cancelledRef.current) setLoading(false)
      })
  }, [])

  useEffect(() => {
    cancelledRef.current = false
    refetch()
    const id = window.setInterval(refetch, FALLBACK_POLL_MS)
    let unlisten: (() => void) | undefined
    listen('skill-candidates-changed', () => refetch())
      .then((fn) => {
        if (cancelledRef.current) fn()
        else unlisten = fn
      })
      .catch(() => { /* demo/browser mode — the fallback poll covers us */ })
    return () => {
      cancelledRef.current = true
      window.clearInterval(id)
      unlisten?.()
    }
  }, [refetch])

  return { candidates, loading, error, refetch }
}
