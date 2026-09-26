// B1 §4-12: in-conversation search (Ctrl+F). Docked at the top of the
// message area: query input, i/n match counter, prev/next navigation
// (Enter / Shift+Enter), close (Esc — also restores composer focus).
//
// Navigation-only by design (approved minimum): Enter jumps the virtualized
// list to the matching message and flashes a transient ring on its bubble;
// the list itself is never filtered.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { RefObject } from 'react'
import type { Virtualizer } from '@tanstack/react-virtual'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import type { ChatMessage } from '@/types'
import { cn } from '@/lib/utils'

interface ChatSearchBarProps {
  messages: ChatMessage[]
  /** True when the list is virtualized (> threshold) — jump via the
   *  virtualizer; otherwise scroll the DOM node into view. */
  virtualized: boolean
  virtualizer: Pick<Virtualizer<HTMLDivElement, Element>, 'scrollToIndex'>
  scrollParentRef: RefObject<HTMLDivElement | null>
  /** Fire when a jump lands (and ~1.2s later with null) so the message
   *  list can ring-highlight the landing bubble. */
  onFlash: (messageIndex: number | null) => void
  onClose: () => void
}

export default function ChatSearchBar({
  messages,
  virtualized,
  virtualizer,
  scrollParentRef,
  onFlash,
  onClose,
}: ChatSearchBarProps) {
  const t = useT()
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  // Case-insensitive substring over user + assistant text (tool/system
  // payloads are chrome, not conversation).
  const matches = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return [] as number[]
    const out: number[] = []
    messages.forEach((m, i) => {
      if ((m.role === 'user' || m.role === 'assistant') && m.content.toLowerCase().includes(q)) {
        out.push(i)
      }
    })
    return out
  }, [messages, query])

  const hasMatches = matches.length > 0
  const activeIndex = hasMatches ? Math.min(active, matches.length - 1) : 0

  const scrollToMessage = useCallback((messageIndex: number) => {
    if (virtualized) {
      virtualizer.scrollToIndex(messageIndex, { align: 'center' })
      return
    }
    const el = scrollParentRef.current?.querySelector<HTMLElement>(`[data-message-index="${messageIndex}"]`)
    el?.scrollIntoView({ block: 'center' })
  }, [virtualized, virtualizer, scrollParentRef])

  const goTo = useCallback((i: number) => {
    if (matches.length === 0) return
    const wrapped = ((i % matches.length) + matches.length) % matches.length
    const messageIndex = matches[wrapped]
    setActive(wrapped)
    scrollToMessage(messageIndex)
    onFlash(messageIndex)
  }, [matches, scrollToMessage, onFlash])

  // New query → land on the first match. Reset the cursor when matches dry
  // up so a fresh search never starts from a stale position.
  const lastQueryRef = useRef(query)
  useEffect(() => {
    if (lastQueryRef.current === query) return
    lastQueryRef.current = query
    setActive(0)
    if (matches.length > 0) goTo(0)
  }, [query, matches, goTo])

  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  const close = useCallback(() => {
    onFlash(null)
    onClose()
    // Esc/close hands focus back to the composer — the search bar stole it
    // from wherever the user was typing.
    window.dispatchEvent(new Event('shannon:focus-composer'))
  }, [onFlash, onClose])

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      goTo(activeIndex + (e.shiftKey ? -1 : 1))
    } else if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      close()
    }
  }

  return (
    <div
      role="search"
      data-testid="chat-search-bar"
      className="shrink-0 z-raised mx-lg mt-md px-sm py-xs rounded-xl glass-surface border border-outline-variant/30 flex items-center gap-sm"
    >
      <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">search</span>
      <input
        ref={inputRef}
        value={query}
        onChange={e => setQuery(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder={t('chat.search.placeholder')}
        aria-label={t('chat.search.aria')}
        className="flex-1 min-w-0 bg-transparent border-none outline-none focus:ring-0 font-label-md text-on-surface placeholder:text-on-surface-variant/60"
      />
      <span
        data-testid="chat-search-count"
        aria-live="polite"
        className={cn('font-mono text-label-xs tabular-nums shrink-0', hasMatches ? 'text-on-surface-variant' : 'text-error')}
      >
        {hasMatches ? `${activeIndex + 1}/${matches.length}` : '0'}
      </span>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={t('chat.search.prev.aria')}
        title={t('chat.search.prev.aria')}
        disabled={!hasMatches}
        className="shrink-0 disabled:opacity-40"
        onClick={() => goTo(activeIndex - 1)}
      >
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">keyboard_arrow_up</span>
      </Button>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={t('chat.search.next.aria')}
        title={t('chat.search.next.aria')}
        disabled={!hasMatches}
        className="shrink-0 disabled:opacity-40"
        onClick={() => goTo(activeIndex + 1)}
      >
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">keyboard_arrow_down</span>
      </Button>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={t('chat.search.close.aria')}
        title={t('chat.search.close.aria')}
        className="shrink-0"
        onClick={close}
      >
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
      </Button>
    </div>
  )
}
