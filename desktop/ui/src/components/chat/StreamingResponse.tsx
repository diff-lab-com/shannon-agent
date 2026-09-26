import { useIntl } from 'react-intl'
import { Markdown } from '@/components/chat/Markdown'
import { SubagentBlock, ToolCallDisplay } from '@/components/chat/MessageBubble'
import { Reasoning } from '@/components/ai-elements'
import type { ReactNode } from 'react'
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

/* B0 P1-1: the old near-bottom auto-scroll guard here was dead code — this
 * component's inner div has no height constraint and never scrolls; the
 * scroll parent is Chat.tsx's message container, which now owns near-bottom
 * tracking itself. Only the visuals remain (bubble, tool cards, typing
 * cursor); "back to live output" is MessageArea's scroll-to-latest FAB. */

/* B2 P2-17: the whole streaming log used to sit in aria-live="polite"
 * (role="log"), re-announcing every token. Announcements are now state
 * transitions only, handled by MessageArea's StreamStatusRegion ("generating
 * …" on start, "reply complete" on end) — this component carries no live
 * region of its own. */

export default function StreamingResponse({
  streamingText,
  thinkingText,
  activeToolCalls,
  onViewDiff,
  headerSlot,
}: StreamingResponseProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  return (
    <div className="relative" role="presentation">
      {headerSlot}
      <div className="flex gap-md max-w-[90%] pt-lg">
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
            tc.tool_name === 'agent_spawn' ? (
              // P1-⑥: sub-agent spawns render as first-class blocks in the
              // live stream too.
              <SubagentBlock key={tc.tool_use_id} toolCall={tc} />
            ) : (
              <ToolCallDisplay key={tc.tool_use_id} toolCall={tc} onViewDiff={onViewDiff} />
            )
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
    </div>
  )
}
