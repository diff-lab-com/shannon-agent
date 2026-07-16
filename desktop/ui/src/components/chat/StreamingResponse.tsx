import { useIntl } from 'react-intl'
import { Markdown } from '@/components/chat/Markdown'
import { ToolCallDisplay } from '@/components/chat/MessageBubble'
import type { ToolCall } from '@/types'

interface StreamingResponseProps {
  streamingText: string
  thinkingText: string
  activeToolCalls: ToolCall[]
  onViewDiff: (path: string) => void
}

export default function StreamingResponse({
  streamingText,
  thinkingText,
  activeToolCalls,
  onViewDiff,
}: StreamingResponseProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  return (
    <div className="flex gap-md max-w-[90%] pt-lg" aria-live="polite" aria-label={t('chat.streaming.aria')}>
      <div className="h-10 w-10 rounded-full bg-primary-container flex items-center justify-center shrink-0 shadow-md">
        <span className="material-symbols-outlined text-on-primary-container">smart_toy</span>
      </div>
      <div className="space-y-md flex-1">
        {thinkingText && (
          <div className="bg-surface-container-lowest p-md rounded-xl border border-outline-variant/10">
            <div className="relative pl-6">
              <div className="absolute left-0 top-1 h-4 w-4 rounded-full bg-primary/20 flex items-center justify-center">
                <div className="h-1.5 w-1.5 rounded-full bg-primary animate-pulse"></div>
              </div>
              <span className="font-label-sm text-on-surface-variant block uppercase opacity-70">{t('chat.streaming.thinking')}</span>
              <p className="text-body-sm whitespace-pre-wrap">{thinkingText}</p>
            </div>
          </div>
        )}
        {activeToolCalls.map(tc => (
          <ToolCallDisplay key={tc.tool_use_id} toolCall={tc} onViewDiff={onViewDiff} />
        ))}
        {streamingText && (
          <div className="bg-surface-container-lowest px-lg py-md rounded-2xl rounded-tl-none border border-outline-variant/20 shadow-sm">
            <div className="font-body-md text-on-surface prose prose-sm max-w-none prose-p:my-1 prose-pre:bg-surface-container prose-pre:p-md prose-pre:rounded-lg prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
              <Markdown>{streamingText}</Markdown>
              <span className="inline-block w-2 h-5 bg-primary/60 ml-xs animate-pulse align-text-bottom"></span>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
