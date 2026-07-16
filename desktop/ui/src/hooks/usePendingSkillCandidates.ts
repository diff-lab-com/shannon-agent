import { useCallback, useEffect, useRef, useState } from 'react'
import { listSkillCandidates, type SkillCandidate } from '@/lib/tauri-api'

const POLL_INTERVAL_MS = 30_000

export function usePendingSkillCandidates(): { candidates: SkillCandidate[]; loading: boolean; refetch: () => void } {
  const [candidates, setCandidates] = useState<SkillCandidate[]>([])
  const [loading, setLoading] = useState(true)
  const cancelledRef = useRef(false)

  const refetch = useCallback(() => {
    listSkillCandidates()
      .then((rows) => { if (!cancelledRef.current) setCandidates(rows) })
      .catch(() => { if (!cancelledRef.current) setCandidates([]) })
      .finally(() => { if (!cancelledRef.current) setLoading(false) })
  }, [])

  useEffect(() => {
    cancelledRef.current = false
    refetch()
    const id = window.setInterval(refetch, POLL_INTERVAL_MS)
    return () => {
      cancelledRef.current = true
      window.clearInterval(id)
    }
  }, [refetch])

  return { candidates, loading, refetch }
}
