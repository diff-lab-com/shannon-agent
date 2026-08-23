import type { RefObject } from 'react'
import type { Virtualizer } from '@tanstack/react-virtual'
import { Button } from '@/components/ui/button'
import WelcomeState from '@/components/WelcomeState'
import { MessageBubble } from '@/components/chat/MessageBubble'
import StreamingResponse from '@/components/chat/StreamingResponse'
import type { ChatMessage, ToolCall } from '@/types'

interface MessageAreaProps {
  t: (id: string) => string
  scrollParentRef: RefObject<HTMLDivElement | null>
  messagesEndRef: RefObject<HTMLDivElement | null>
  messages: ChatMessage[]
  streamingText: string
  thinkingText: string
  activeToolCalls: ToolCall[]
  error: string | null
  virtualizer: Virtualizer<HTMLDivElement, Element>
  shouldVirtualize: boolean
  setInput: (s: string) => void
  handleSend: () => void
  setDiffPath: (p: string | null) => void
  setDiffPaths: (p: string[] | null) => void
  input: string
}

// Virtualized + non-virtualized message list. Below the threshold (30
// messages) we render everything in a flat log so jsdom tests still see the
// bubbles — virtualization's measureElement needs a real DOM with height.
export default function MessageArea({
  t,
  scrollParentRef,
  messagesEndRef,
  messages,
  streamingText,
  thinkingText,
  activeToolCalls,
  error,
  virtualizer,
  shouldVirtualize,
  setInput,
  handleSend,
  setDiffPath,
  setDiffPaths,
  input,
}: MessageAreaProps) {
  return (
    <div ref={scrollParentRef} className="flex-1 overflow-y-auto px-xl pt-lg pb-32">
      {messages.length === 0 && !streamingText && (
        <WelcomeState onSelectPrompt={setInput} />
      )}

      {messages.length > 0 && shouldVirtualize && (
        <div
          style={{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }}
          role="log"
          aria-live="polite"
          aria-label={t('chat.history.aria')}
        >
          {virtualizer.getVirtualItems().map(vItem => {
            const msg = messages[vItem.index]
            return (
              <div
                key={`${msg.timestamp}-${vItem.index}`}
                data-index={vItem.index}
                ref={virtualizer.measureElement}
                className="pb-lg"
                style={{ position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${vItem.start}px)` }}
              >
                <MessageBubble message={msg} messageIndex={vItem.index} onViewDiff={setDiffPath} onViewDiffMulti={setDiffPaths} />
              </div>
            )
          })}
        </div>
      )}

      {messages.length > 0 && !shouldVirtualize && (
        <div role="log" aria-live="polite" aria-label={t('chat.history.aria')}>
          {messages.map((msg, i) => (
            <div key={`${msg.timestamp}-${i}`} className="pb-lg">
              <MessageBubble message={msg} messageIndex={i} onViewDiff={setDiffPath} onViewDiffMulti={setDiffPaths} />
            </div>
          ))}
        </div>
      )}

      {/* Streaming response */}
      {(streamingText || thinkingText || activeToolCalls.length > 0) && (
        <StreamingResponse
          streamingText={streamingText}
          thinkingText={thinkingText}
          activeToolCalls={activeToolCalls}
          onViewDiff={setDiffPath}
        />
      )}

      {error && (
        <div className="mx-auto max-w-md p-md bg-error/10 border border-error/20 rounded-xl text-center">
          <p className="text-body-sm text-error">{error}</p>
          <Button variant="ghost" className="mt-sm text-error hover:bg-error/10 text-label-md cursor-pointer" onClick={() => { if (input.trim()) handleSend() }}>{t('chat.error.retry')}</Button>
        </div>
      )}

      <div ref={messagesEndRef} />
    </div>
  )
}