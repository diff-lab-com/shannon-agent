import { useState, useEffect, useCallback } from 'react'

/**
 * Paginate a client-side list with a "show more" affordance. Used by lists
 * that aren't worth a full virtualizer (Skills catalog, History rows) —
 * keeps the DOM small for the common case while still letting users page
 * through everything without an extra network round-trip.
 *
 * The visible count resets whenever `items` changes (new filter, new fetch),
 * so the user always starts at the top of a fresh list.
 */
export function usePagedVisible<T>(items: T[], pageSize: number, initialCount: number = pageSize) {
  const [visible, setVisible] = useState(initialCount)

  useEffect(() => {
    setVisible(initialCount)
  }, [items, initialCount])

  const showMore = useCallback(() => {
    setVisible(v => Math.min(items.length, v + pageSize))
  }, [items.length, pageSize])

  const slice = items.slice(0, visible)
  const hasMore = items.length > visible
  const remaining = Math.max(0, items.length - visible)

  return { slice, hasMore, remaining, showMore, visible }
}
