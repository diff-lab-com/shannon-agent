import { useState, useEffect, useCallback } from 'react'

/**
 * Paginate a client-side list with a "show more" affordance. Used by lists
 * that aren't worth a full virtualizer (Skills catalog, History rows) —
 * keeps the DOM small for the common case while still letting users page
 * through everything without an extra network round-trip.
 *
 * The visible count resets when the list's **length** changes (new fetch, a
 * filter that adds/removes rows) so the user starts at the top of a fresh
 * list. B3 P1-18: the reset used to key on array *identity*, which meant any
 * unmemoized derived list (e.g. Skills' per-render `filtered`) reset the
 * count on every render — "Show more" bounced straight back inside search,
 * the one context where paging matters most.
 */
export function usePagedVisible<T>(items: T[], pageSize: number, initialCount: number = pageSize) {
  const [visible, setVisible] = useState(initialCount)

  useEffect(() => {
    setVisible(initialCount)
  }, [items.length, initialCount])

  const showMore = useCallback(() => {
    setVisible(v => Math.min(items.length, v + pageSize))
  }, [items.length, pageSize])

  const slice = items.slice(0, visible)
  const hasMore = items.length > visible
  const remaining = Math.max(0, items.length - visible)

  return { slice, hasMore, remaining, showMore, visible }
}
