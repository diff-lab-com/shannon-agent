import { memo } from 'react'
import { useIntl } from 'react-intl'
import { Markdown } from '@/components/chat/Markdown'
import { splitStreamingMarkdown } from '@/lib/streamingMarkdown'
import { SubagentBlock, ToolCallDisplay } from '@/components/chat/MessageBubble'
import { Reasoning } from '@/components/ai-elements'
import { readShowThinkingPref } from '@/lib/thinkingPref'
import type { ToolCall } from '@/types'

interface StreamingResponseProps {
  streamingText: string
  thinkingText: string
  activeToolCalls: ToolCall[]
  onViewDiff: (path: string) => void
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

/* B3-2 (§三 P1-6): the streaming body used to re-parse the ENTIRE accumulated
 * text on every ~50ms flush — remark-gfm + remark-math + rehype-highlight +
 * sanitize + katex over thousands of tokens, O(n²) as the reply grows. The
 * text is now cut at its last safe blank-line boundary (a boundary that is
 * provably not inside an open fence/`$$` math block/loose list/indented-code
 * interior — see lib/streamingMarkdown.ts) into:
 *   - a finalized prefix, rendered by the memoized component below. While the
 *     stream appends, the prefix string is value-stable, so React.memo bails
 *     and NONE of the heavy pipeline runs over it again (no length/hash key
 *     needed — default shallow props compare already gives exactly that);
 *   - an active tail, re-parsed each flush, whose size stays at "the last
 *     paragraph (or open block)" instead of the whole reply.
 * Both halves ride `deferHighlight` (syntax coloring returns on the
 * finalized MessageBubble render); paragraphs join seamlessly because the
 * prefix keeps its trailing blank run, so the DOM is sibling <p> blocks
 * exactly as a single parse would produce. */

/**
 * B3-2: memoized renderer for the finalized prefix. Re-renders only when the
 * prefix itself grows (a new paragraph finalized) — tail updates are invisible
 * to it, which is what keeps the per-flush cost bounded.
 */
const FinalizedMarkdown = memo(function FinalizedMarkdown({ text }: { text: string }) {
  return <Markdown deferHighlight>{text}</Markdown>
})

export default function StreamingResponse({
  streamingText,
  thinkingText,
  activeToolCalls,
  onViewDiff,
}: StreamingResponseProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  // B3-2: pure per-render split (cheap line scan); prefix is '' until the
  // first paragraph boundary finalizes.
  const { prefix, tail } = splitStreamingMarkdown(streamingText)
  // Settings R3 T9: 'none' hides thinking completely — including live
  // (streaming) output. 'first'/'all' both stream as usual: the in-flight
  // run IS the current turn, so its thinking is that turn's first block.
  const showThinking = readShowThinkingPref() !== 'none'

  return (
    <div className="relative" role="presentation">
      <div className="flex gap-md max-w-[90%] pt-lg">
        <div className="h-10 w-10 rounded-full bg-primary-container flex items-center justify-center shrink-0 shadow-e2">
          <span className="material-symbols-outlined text-on-primary-container">smart_toy</span>
        </div>
        <div className="space-y-md flex-1">
          {showThinking && thinkingText && (
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
            <div className="bg-surface-container-lowest px-lg py-md rounded-2xl rounded-tl-none border border-outline-variant/20 shadow-e1">
              <div className="font-body-md text-on-surface prose prose-sm max-w-none prose-p:my-xs prose-pre:bg-surface-container prose-pre:p-md prose-pre:rounded-lg prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
                {prefix && <FinalizedMarkdown text={prefix} />}
                <Markdown deferHighlight>{tail}</Markdown>
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
