import { useEffect, useRef, useState, useCallback, type ReactNode } from 'react'
import { useIntl } from 'react-intl'
import { Markdown } from '@/components/chat/Markdown'
import { ToolCallDisplay } from '@/components/chat/MessageBubble'
import { Reasoning } from '@/components/ai-elements'
import type { ToolCall } from '@/types'

interface StreamingResponseProps {
  streamingText: string
  thinkingText: string
  activeToolCalls: ToolCall[]
  onViewDiff: (path: string) => void
  /** Slots for extra content above/below the streaming bubble (e.g.
   *  prepended regeneration blocks). Default empty. */
  headerSlot?: ReactNode
}

/* Threshold below which auto-scroll keeps the bubble glued to the
 * bottom of the viewport. Above the threshold the user is treated as
 * having scrolled away and the "scroll-to-bottom" button is shown. */
const SCROLL_AWAY_THRESHOLD_PX = 80

export default function StreamingResponse({
  streamingText,
  thinkingText,
  activeToolCalls,
  onViewDiff,
  headerSlot,
}: StreamingResponseProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const scrollRef = useRef<HTMLDivElement>(null)
  const [showJumpToBottom, setShowJumpToBottom] = useState(false)

  // The first user-driven scroll up after a stream begins should keep
  // position stable (don't jerk the viewport). We track this with a
  // ref to avoid re-renders on every wheel event.
  const programmaticScrollRef = useRef(false)
  const lastContentRef = useRef('')

  const scrollToBottom = useCallback((smooth = false) => {
    const el = scrollRef.current
    if (!el) return
    programmaticScrollRef.current = true
    el.scrollTo({
      top: el.scrollHeight,
      behavior: smooth ? 'smooth' : 'auto',
    })
    // Reset the flag after the scroll settles
    requestAnimationFrame(() => {
      programmaticScrollRef.current = false
    })
  }, [])

  // Auto-scroll while streaming — but only if the user is already near
  // the bottom. If they scrolled away, leave them alone.
  useEffect(() => {
    const el = scrollRef.current
    if (!el) return

    // On the very first content arrival, jump to bottom unconditionally
    // so the user actually sees the response (in case the viewport
    // scrolled because of a previous long response).
    const isFirstContent = lastContentRef.current === '' && (streamingText || thinkingText)
    lastContentRef.current = streamingText + thinkingText

    if (isFirstContent) {
      scrollToBottom(false)
      return
    }

    // Subsequent updates: only auto-scroll if near bottom
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight
    if (distanceFromBottom < SCROLL_AWAY_THRESHOLD_PX) {
      scrollToBottom(true)
    }
  }, [streamingText, thinkingText, scrollToBottom])

  const handleScroll = () => {
    const el = scrollRef.current
    if (!el || programmaticScrollRef.current) return
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight
    setShowJumpToBottom(distanceFromBottom > SCROLL_AWAY_THRESHOLD_PX)
  }

  return (
    <div className="relative" role="presentation">
      {headerSlot}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="flex gap-md max-w-[90%] pt-lg overflow-y-auto"
        aria-live="polite"
        aria-label={t('chat.streaming.aria')}
        role="log"
      >
        <div className="h-10 w-10 rounded-full bg-primary-container flex items-center justify-center shrink-0 shadow-md">
          <span className="material-symbols-outlined text-on-primary-container">smart_toy</span>
        </div>
        <div className="space-y-md flex-1">
          {thinkingText && (
            <Reasoning header={t('chat.streaming.thinking')} defaultOpen={false}>
              <p className="whitespace-pre-wrap">{thinkingText}</p>
            </Reasoning>
          )}
          {activeToolCalls.map(tc => (
            <ToolCallDisplay key={tc.tool_use_id} toolCall={tc} onViewDiff={onViewDiff} />
          ))}
          {streamingText && (
            <div className="bg-surface-container-lowest px-lg py-md rounded-2xl rounded-tl-none border border-outline-variant/20 shadow-sm">
              <div className="font-body-md text-on-surface prose prose-sm max-w-none prose-p:my-1 prose-pre:bg-surface-container prose-pre:p-md prose-pre:rounded-lg prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
                <Markdown>{streamingText}</Markdown>
                {/* P2-5d typing cursor — CSS-driven (not a moving dot) so it
                    matches Claude Desktop's style. */}
                <span
                  aria-hidden="true"
                  className="streaming-cursor inline-block w-[7px] h-[1em] ml-[2px] bg-primary align-text-bottom rounded-sm"
                />
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Smart jump-to-bottom — appears only when the user scrolled away. */}
      {showJumpToBottom && (
        <button
          type="button"
          onClick={() => scrollToBottom(true)}
          aria-label={t('chat.streaming.jumpToBottom')}
          className="absolute bottom-xs right-sm flex items-center gap-xs px-sm py-xs rounded-full bg-surface-container-high border border-outline-variant/30 text-on-surface-variant hover:text-primary shadow-e3 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 transition-colors"
        >
          <span className="material-symbols-outlined icon-sm">arrow_downward</span>
          <span className="text-label-sm">{t('chat.streaming.jumpToBottom')}</span>
        </button>
      )}
    </div>
  )
}
