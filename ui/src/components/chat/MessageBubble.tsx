import { useState, useRef, memo } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { useApp } from '@/context/AppContext'
import { useModalFocus } from '@/hooks/useModalFocus'
import * as api from '@/lib/tauri-api'
import { Markdown } from '@/components/chat/Markdown'
import { FootnoteMarkdown } from '@/components/chat/FootnoteMarkdown'
import {
  Message,
  MessageAvatar,
  MessageContent,
  ResponseStream,
  ActionToolbar,
  Tool,
  ToolHeader,
  ToolContent,
} from '@/components/ai-elements'
import { ResearchReportModal } from '@/components/chat/ResearchReportModal'
import type { ChatMessage, ToolCall, FileAttachment } from '@/types'

interface MessageBubbleProps {
  message: ChatMessage
  messageIndex: number
  isBranch?: boolean
  onViewDiff: (path: string) => void
}

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'])

function isImagePath(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase()
  return ext != null && IMAGE_EXTENSIONS.has(ext)
}

function AttachmentPreview({ attachment }: { attachment: FileAttachment }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [open, setOpen] = useState(false)
  const modalRef = useRef<HTMLDivElement>(null)
  useModalFocus(open, modalRef)
  const isImage = isImagePath(attachment.path)

  const handleClick = () => setOpen(true)

  return (
    <>
      <button
        type="button"
        onClick={handleClick}
        className="group/att inline-flex items-center gap-xs px-sm py-xs bg-surface-container-low hover:bg-surface-container border border-outline-variant/30 rounded-lg text-on-surface-variant hover:text-primary transition-colors cursor-pointer"
        title={attachment.path}
        aria-label={t('chat.message.attachment.open')}
      >
        {isImage ? (
          <img
            src={convertFileSrc(attachment.path)}
            alt={attachment.name}
            className="h-8 w-8 rounded object-cover shrink-0"
            onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none' }}
          />
        ) : (
          <span className="material-symbols-outlined text-[18px]">description</span>
        )}
        <span className="font-label-sm max-w-[160px] truncate">{attachment.name}</span>
      </button>

      {open && (
        <div
          ref={modalRef}
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-lg"
          onClick={() => setOpen(false)}
          role="dialog"
          aria-modal="true"
          aria-label={attachment.name}
        >
          {isImage ? (
            <img
              src={convertFileSrc(attachment.path)}
              alt={attachment.name}
              className="max-h-[90vh] max-w-[90vw] object-contain rounded-lg shadow-2xl"
              onClick={(e) => e.stopPropagation()}
            />
          ) : (
            <div
              className="bg-surface-container-lowest rounded-xl p-lg shadow-2xl max-w-md"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="flex items-center gap-sm mb-md">
                <span className="material-symbols-outlined text-on-surface-variant">description</span>
                <span className="font-label-md text-on-surface truncate">{attachment.name}</span>
              </div>
              <p className="text-body-sm text-on-surface-variant mb-md break-all">{attachment.path}</p>
              <Button
                onClick={() => {
                  window.open(convertFileSrc(attachment.path), '_blank')
                }}
              >
                <span className="material-symbols-outlined text-[18px] mr-xs">open_in_new</span>
                {t('chat.message.attachment.openExternally')}
              </Button>
            </div>
          )}
          <button
            type="button"
            onClick={() => setOpen(false)}
            aria-label={t('chat.message.attachment.close')}
            className="absolute top-md right-md text-on-surface-variant hover:text-on-surface bg-surface-container-lowest/80 rounded-full p-sm"
          >
            <span className="material-symbols-outlined">close</span>
          </button>
        </div>
      )}
    </>
  )
}

export const MessageBubble = memo(function MessageBubble({ message, messageIndex, isBranch, onViewDiff }: MessageBubbleProps) {
  const isUser = message.role === 'user'
  const [liked, setLiked] = useState(false)
  const [isBranching, setIsBranching] = useState(false)
  const [reportOpen, setReportOpen] = useState(false)
  const { sendMessage, currentSessionId, switchSession, refreshSessions } = useApp()
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const handleCopy = () => {
    navigator.clipboard.writeText(message.content).catch(() => toast.error(t('chat.toast.copyFailed')))
  }

  const handleRegenerate = () => {
    sendMessage('Regenerate the previous response').catch(() => toast.error(t('chat.toast.regenerateFailed')))
  }

  const handleBranch = async () => {
    if (!currentSessionId || isBranching) return

    const confirmed = window.confirm(t('chat.message.branch.confirm'))
    if (!confirmed) return

    setIsBranching(true)
    try {
      const newSession = await api.branchSession(currentSessionId, messageIndex)
      await refreshSessions()
      await switchSession(newSession.id)
      toast.success(t('chat.message.branch.success'))
    } catch (error) {
      console.error('Branch failed:', error)
      toast.error(t('chat.message.branch.failed'))
    } finally {
      setIsBranching(false)
    }
  }

  const hasAttachments = message.file_attachments && message.file_attachments.length > 0
  const hasReport = !!message.research_report

  if (isUser) {
    return (
      <Message from="user" className="flex justify-end">
        <MessageContent className="max-w-[80%]">
          {isBranch && (
            <div className="flex items-center gap-xs mb-xs justify-end">
              <span className="material-symbols-outlined text-[14px] text-on-surface-variant/50">fork_right</span>
              <span className="font-label-sm text-on-surface-variant/50">{t('chat.message.branch')}</span>
            </div>
          )}
          {hasAttachments && (
            <div className="flex flex-wrap gap-xs mb-xs justify-end">
              {message.file_attachments!.map((att, i) => (
                <AttachmentPreview key={i} attachment={att} />
              ))}
            </div>
          )}
          <div className="bg-primary-fixed text-on-primary-fixed px-lg py-md rounded-2xl rounded-tr-none shadow-sm">
            <p className="font-body-md whitespace-pre-wrap">{message.content}</p>
          </div>
          <ActionToolbar className="gap-sm mt-xs justify-end">
            <Button
              aria-label={t('chat.message.branch.aria')}
              onClick={handleBranch}
              disabled={isBranching || !currentSessionId}
              className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors"
              title={t('chat.message.branch.button')}
            >
              <span className="material-symbols-outlined text-[18px]" aria-hidden="true">
                {isBranching ? 'hourglass_empty' : 'fork_right'}
              </span>
            </Button>
          </ActionToolbar>
        </MessageContent>
      </Message>
    )
  }

  return (
    <Message from="assistant" className="flex gap-md max-w-[90%]">
      <MessageAvatar from="assistant" />
      <MessageContent className="space-y-md flex-1">
        <div className="bg-surface-container-lowest px-lg py-md rounded-2xl rounded-tl-none border border-outline-variant/20 shadow-sm">
          <ResponseStream className="font-body-md text-on-surface prose prose-sm max-w-none prose-p:my-1 prose-pre:bg-surface-container prose-pre:p-md prose-pre:rounded-lg prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
            <FootnoteMarkdown>{message.content}</FootnoteMarkdown>
          </ResponseStream>
          {message.tool_calls && message.tool_calls.length > 0 && (
            <div className="mt-md space-y-sm">
              {message.tool_calls.map(tc => (
                <ToolCallDisplay key={tc.tool_use_id} toolCall={tc} onViewDiff={onViewDiff} />
              ))}
            </div>
          )}
        </div>
        <ActionToolbar>
          <Button aria-label={t('chat.message.like.aria')} aria-pressed={liked} onClick={() => setLiked(!liked)} className={`flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container transition-colors ${liked ? 'text-primary' : 'text-on-surface-variant'}`}>
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">{liked ? 'thumb_up' : 'thumb_up_off_alt'}</span>
          </Button>
          <Button aria-label={t('chat.message.copy.aria')} onClick={handleCopy} className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors">
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">content_copy</span>
          </Button>
          <Button aria-label={t('chat.message.regenerate.aria')} onClick={handleRegenerate} className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors">
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">refresh</span>
          </Button>
          {hasReport && (
            <Button
              aria-label={t('chat.message.report.aria')}
              onClick={() => setReportOpen(true)}
              className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors"
            >
              <span className="material-symbols-outlined text-[18px]" aria-hidden="true">article</span>
              <span className="text-label-sm">{t('chat.message.report')}</span>
            </Button>
          )}
        </ActionToolbar>
        {hasReport && (
          <ResearchReportModal
            report={message.research_report!}
            open={reportOpen}
            onClose={() => setReportOpen(false)}
          />
        )}
      </MessageContent>
    </Message>
  )
})

const FILE_MUTATING_TOOLS = new Set(['write_file', 'edit_file', 'apply_patch', 'str_replace_editor', 'replace'])

function extractFilePath(toolName: string, input: unknown): string | null {
  if (!input || typeof input !== 'object') return null
  const obj = input as Record<string, unknown>
  const raw = typeof obj.path === 'string' ? obj.path
    : typeof obj.file_path === 'string' ? obj.file_path
    : typeof obj.filePath === 'string' ? obj.filePath
    : null
  if (!raw) return null
  return FILE_MUTATING_TOOLS.has(toolName) ? raw : null
}

export const ToolCallDisplay = memo(function ToolCallDisplay({ toolCall, onViewDiff }: { toolCall: ToolCall; onViewDiff: (path: string) => void }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [expanded, setExpanded] = useState(false)
  const statusIcon = toolCall.status === 'running' ? 'hourglass_empty' : toolCall.status === 'error' ? 'error' : 'check_circle'
  const statusColor = toolCall.status === 'running' ? 'text-secondary' : toolCall.status === 'error' ? 'text-error' : 'text-tertiary'
  const filePath = extractFilePath(toolCall.tool_name, toolCall.tool_input)
  const canDiff = filePath != null && toolCall.status === 'completed' && !toolCall.is_error

  return (
    <Tool name={toolCall.tool_name} status={toolCall.status} className="p-sm">
      <ToolHeader onClick={() => setExpanded(!expanded)}>
        <span className={`material-symbols-outlined text-[16px] ${statusColor} ${toolCall.status === 'running' ? 'animate-spin' : ''}`}>{statusIcon}</span>
        <span className="font-label-md text-on-surface flex-1 truncate">{toolCall.tool_name}</span>
        {canDiff && (
          <button
            type="button"
            aria-label={intl.formatMessage({ id: 'chat.message.diff.aria' }, { path: filePath })}
            className="flex items-center gap-xs px-xs py-[2px] rounded-md text-tertiary hover:bg-tertiary-container/40 text-[11px] cursor-pointer"
            onClick={(e) => { e.stopPropagation(); onViewDiff(filePath!) }}
          >
            <span className="material-symbols-outlined text-[14px]">difference</span>
            {t('chat.message.diff')}
          </button>
        )}
        <span className="material-symbols-outlined text-[16px] text-on-surface-variant">{expanded ? 'expand_less' : 'expand_more'}</span>
      </ToolHeader>
      {expanded && (
        <ToolContent>
          {toolCall.tool_input ? (
            <pre className="text-body-sm text-on-surface-variant bg-surface-container p-sm rounded-lg overflow-x-auto max-h-[200px]">{JSON.stringify(toolCall.tool_input ?? null, null, 2)}</pre>
          ) : null}
          {toolCall.result && (
            toolCall.is_error ? (
              <pre className="text-body-sm p-sm rounded-lg overflow-x-auto max-h-[200px] bg-error/5 text-error">{toolCall.result}</pre>
            ) : (
              <div className="text-body-sm p-sm rounded-lg overflow-x-auto max-h-[200px] bg-surface-container text-on-surface-variant prose prose-sm max-w-none prose-pre:bg-surface-container-lowest prose-pre:p-sm prose-pre:rounded prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
                <Markdown>{toolCall.result}</Markdown>
              </div>
            )
          )}
        </ToolContent>
      )}
    </Tool>
  )
})
