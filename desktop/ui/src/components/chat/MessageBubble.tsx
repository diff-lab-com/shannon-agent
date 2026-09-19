import { useState, memo } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { messageFeedbackKey } from '@/lib/feedbackKey'
import type { FeedbackRating } from '@/lib/tauri-api'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { Modal } from '@/components/ui/modal'
import { useChat } from '@/context/ChatContext'
import { useSessions } from '@/context/SessionContext'
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
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { ResearchReportModal } from '@/components/chat/ResearchReportModal'
import { ArtifactChipList } from '@/components/artifact/ArtifactChip'
import { detectArtifacts } from '@/components/artifact/detectArtifact'
import type { ChatMessage, ToolCall, FileAttachment } from '@/types'
import { cn } from '@/lib/utils'

interface MessageBubbleProps {
  message: ChatMessage
  messageIndex: number
  isBranch?: boolean
  onViewDiff: (path: string) => void
  onViewDiffMulti?: (paths: string[]) => void
  /** Conversation turn this message owns, when it is rewindable (/rewind). */
  rewindTurnIndex?: number | null
  onRewind?: (turnIndex: number) => Promise<void>
  /** P1-⑤ telemetry: tool_use_id → duration (ms) from the session's L0
   *  trace timeline — the authoritative durations for historical messages
   *  (live tool calls carry their own client-measured duration_ms). */
  durationLookup?: Map<string, number>
}

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'])

function isImagePath(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase()
  return ext != null && IMAGE_EXTENSIONS.has(ext)
}

/* ─────── Header — avatar + role + timestamp ─────── */

function MessageHeader({
  role,
  timestamp,
  isBranch,
}: {
  role: 'user' | 'assistant' | 'tool'
  timestamp?: number
  isBranch?: boolean
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const label =
    role === 'user' ? t('chat.message.header.user')
      : role === 'tool' ? t('chat.message.header.tool')
      : t('chat.message.header.assistant')
  const time = timestamp ? new Date(timestamp).toLocaleTimeString(intl.locale, { hour: '2-digit', minute: '2-digit' }) : ''
  return (
    <div className="flex items-center gap-xs text-label-xs text-on-surface-variant mb-xs" aria-hidden="true">
      <span className="font-label-xs uppercase tracking-wide font-medium">{label}</span>
      {isBranch && (
        <>
          <span aria-hidden="true">·</span>
          <span className="material-symbols-outlined text-[12px]">fork_right</span>
          <span>{t('chat.message.branch')}</span>
        </>
      )}
      {time && (
        <>
          <span aria-hidden="true">·</span>
          <time dateTime={new Date(timestamp!).toISOString()} className="font-mono">{time}</time>
        </>
      )}
    </div>
  )
}

function AttachmentPreview({ attachment }: { attachment: FileAttachment }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [open, setOpen] = useState(false)
  const isImage = isImagePath(attachment.path)

  const handleClick = () => setOpen(true)

  return (
    <>
      <Button
        variant="outline"
        onClick={handleClick}
        className="group/att inline-flex items-center gap-xs px-sm py-xs h-auto bg-surface-container-low hover:bg-surface-container text-on-surface-variant hover:text-primary"
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
      </Button>

      <Modal
        open={open}
        onClose={() => setOpen(false)}
        size="full"
        showCloseButton={false}
        title={attachment.name}
        closeLabel={t('chat.message.attachment.close')}
        className="bg-black/70 backdrop-blur-sm p-lg"
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
        <Button
          variant="ghost"
          onClick={() => setOpen(false)}
          aria-label={t('chat.message.attachment.close')}
          className="absolute top-md right-md text-on-surface-variant hover:text-on-surface bg-surface-container-lowest/80 rounded-full p-sm"
        >
          <span className="material-symbols-outlined">close</span>
        </Button>
      </Modal>
    </>
  )
}

export const MessageBubble = memo(function MessageBubble({ message, messageIndex, isBranch, onViewDiff, onViewDiffMulti, rewindTurnIndex, onRewind, durationLookup }: MessageBubbleProps) {
  const isUser = message.role === 'user'
  const [isBranching, setIsBranching] = useState(false)
  const [pendingBranch, setPendingBranch] = useState(false)
  const [pendingRewind, setPendingRewind] = useState(false)
  const [isRewinding, setIsRewinding] = useState(false)
  const [reportOpen, setReportOpen] = useState(false)
  const { sendMessage, feedback, recordFeedback } = useChat()
  const { currentSessionId, switchSession, refreshSessions } = useSessions()
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  // PM-12: the rating lives in the persisted per-session store, not in a
  // local useState that died with the component tree.
  const feedbackKey = messageFeedbackKey(message.timestamp, message.content)
  const myRating = feedback[feedbackKey] ?? null

  const toggleFeedback = (rating: FeedbackRating) => {
    recordFeedback(feedbackKey, myRating === rating ? null : rating)
      .catch((e) => toastError(t('chat.message.feedback.failed'), e))
  }

  const handleCopy = () => {
    navigator.clipboard.writeText(message.content).catch((e) => toastError(t('chat.toast.copyFailed'), e))
  }

  const handleRegenerate = () => {
    sendMessage(t('chat.regenerate.prompt')).catch((e) => toastError(t('chat.toast.regenerateFailed'), e))
  }

  const confirmRewind = async () => {
    if (rewindTurnIndex == null || !onRewind) return
    setPendingRewind(false)
    setIsRewinding(true)
    try {
      await onRewind(rewindTurnIndex)
      toast.success(t('chat.message.rewind.success'))
    } catch (error) {
      toastError(t('chat.message.rewind.failed'), error)
    } finally {
      setIsRewinding(false)
    }
  }

  const handleBranch = () => {
    if (!currentSessionId || isBranching) return
    setPendingBranch(true)
  }

  const confirmBranch = async () => {
    if (!currentSessionId) return
    setPendingBranch(false)
    setIsBranching(true)
    try {
      const newSession = await api.branchSession(currentSessionId, messageIndex)
      await refreshSessions()
      await switchSession(newSession.id)
      toast.success(t('chat.message.branch.success'))
    } catch (error) {
      toastError(t('chat.message.branch.failed'), error)
    } finally {
      setIsBranching(false)
    }
  }

  const hasAttachments = message.file_attachments && message.file_attachments.length > 0
  const hasReport = !!message.research_report
  const detectedArtifacts = !isUser ? detectArtifacts(message.content) : []

  if (isUser) {
    return (
      <>
      <Message from="user" className="flex justify-end">
        <MessageContent className="max-w-[80%]">
          <MessageHeader role="user" timestamp={message.timestamp} isBranch={isBranch} />
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
          <ActionToolbar className="gap-sm mt-xs justify-end opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity">
            <Button
              aria-label={t('chat.message.copy.aria')}
              onClick={handleCopy}
              className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            >
              <span className="material-symbols-outlined text-[18px]" aria-hidden="true">content_copy</span>
            </Button>
            <Button
              aria-label={t('chat.message.branch.aria')}
              onClick={handleBranch}
              disabled={isBranching || !currentSessionId}
              className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
              title={t('chat.message.branch.button')}
            >
              <span className="material-symbols-outlined text-[18px]" aria-hidden="true">
                {isBranching ? 'hourglass_empty' : 'fork_right'}
              </span>
            </Button>
            {rewindTurnIndex != null && (
              <Button
                aria-label={t('chat.message.rewind.aria')}
                onClick={() => setPendingRewind(true)}
                disabled={isRewinding}
                className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                title={t('chat.message.rewind.button')}
              >
                <span className="material-symbols-outlined text-[18px]" aria-hidden="true">
                  {isRewinding ? 'hourglass_empty' : 'undo'}
                </span>
              </Button>
            )}
          </ActionToolbar>
        </MessageContent>
      </Message>
      <ConfirmDialog
        open={pendingBranch}
        title={t('chat.message.branch.confirm.title')}
        message={t('chat.message.branch.confirm.message')}
        confirmLabel={t('chat.message.branch.button')}
        cancelLabel={t('chat.message.branch.confirm.cancel')}
        busy={isBranching}
        onConfirm={() => void confirmBranch()}
        onCancel={() => setPendingBranch(false)}
      />
      <ConfirmDialog
        open={pendingRewind}
        title={t('chat.message.rewind.confirm.title')}
        message={t('chat.message.rewind.confirm.message')}
        confirmLabel={t('chat.message.rewind.confirm.confirm')}
        cancelLabel={t('chat.message.rewind.confirm.cancel')}
        busy={isRewinding}
        onConfirm={() => void confirmRewind()}
        onCancel={() => setPendingRewind(false)}
      />
    </>
  )
}

  /* Assistant + Tool variants — share most of the chrome; tool messages
   * get a slightly muted style and don't carry the like / regen /
   * report buttons (those actions only make sense for assistant text). */
  const isTool = message.role === 'tool'

  return (
    <Message from={isTool ? 'system' : 'assistant'} className="flex gap-md max-w-[90%] group">
      <MessageAvatar from="assistant" icon={isTool ? 'build' : 'smart_toy'} />
      <MessageContent className="space-y-md flex-1">
        <MessageHeader role={isTool ? 'tool' : 'assistant'} timestamp={message.timestamp} />
        <div className="bg-surface-container-lowest px-lg py-md rounded-2xl rounded-tl-none border border-outline-variant/20 shadow-sm min-w-0 overflow-x-auto">
          <ResponseStream className="font-body-md text-on-surface prose prose-sm max-w-none prose-p:my-1 prose-pre:bg-surface-container prose-pre:p-md prose-pre:rounded-lg prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
            <FootnoteMarkdown>{message.content}</FootnoteMarkdown>
          </ResponseStream>
          {detectedArtifacts.length > 0 && (
            <div className="mt-md">
              <ArtifactChipList artifacts={detectedArtifacts} />
            </div>
          )}
          {message.tool_calls && message.tool_calls.length > 0 && (
            <div className="mt-md space-y-sm">
              {(() => {
                const changedPaths = message.tool_calls
                  .filter(tc => tc.status === 'completed' && !tc.is_error)
                  .map(tc => extractFilePath(tc.tool_name, tc.tool_input))
                  .filter((p): p is string => p != null)
                const uniquePaths = Array.from(new Set(changedPaths))
                return uniquePaths.length > 0 ? (
                  <div className="flex items-center justify-between gap-sm px-md py-xs rounded-lg bg-tertiary/5 border border-tertiary/20">
                    <div className="flex items-center gap-sm min-w-0">
                      <span className="material-symbols-outlined icon-sm text-tertiary shrink-0">difference</span>
                      <span className="font-label-sm text-on-surface truncate">
                        {intl.formatMessage(
                          { id: 'chat.message.filesChanged' },
                          { count: uniquePaths.length }
                        )}
                      </span>
                      <span className="font-label-xs text-on-surface-variant truncate font-mono">
                        {uniquePaths.join(', ')}
                      </span>
                    </div>
                    {uniquePaths.length > 1 && (
                      <Button
                        variant="ghost"
                        size="sm"
                        className="shrink-0 gap-xs px-sm py-xs text-tertiary hover:bg-tertiary/10"
                        onClick={() => onViewDiffMulti?.(uniquePaths)}
                      >
                        <span className="material-symbols-outlined icon-sm">open_in_new</span>
                        {t('chat.message.reviewAll')}
                      </Button>
                    )}
                  </div>
                ) : null
              })()}
              {/* P2-⑨ (ZCode delta): a run of consecutive same-tool failures is
                  prefaced by a retry-chain banner linking to the turn timeline
                  — the long-horizon "failed → retried → recovered" narrative
                  stays readable without expanding every card. */}
              {(() => {
                const tcs = message.tool_calls
                const out: React.ReactNode[] = []
                const renderTool = (tc: ToolCall, key: string) =>
                  tc.tool_name === 'agent_spawn' ? (
                    <SubagentBlock key={key} toolCall={tc} />
                  ) : (
                    <ToolCallDisplay
                      key={key}
                      toolCall={tc}
                      onViewDiff={onViewDiff}
                      durationMs={durationLookup?.get(tc.tool_use_id)}
                    />
                  )
                let i = 0
                while (i < tcs.length) {
                  const tc = tcs[i]
                  if (tc.status === 'error') {
                    let j = i
                    while (
                      j + 1 < tcs.length &&
                      tcs[j + 1].status === 'error' &&
                      tcs[j + 1].tool_name === tc.tool_name
                    ) { j++ }
                    const chainLen = j - i + 1
                    if (chainLen >= 2) {
                      // D7: inline each attempt's first error line so the
                      // failure→retry→recovery narrative reads without
                      // expanding every card.
                      const reasons = tcs.slice(i, j + 1).map(tc => {
                        const line = (tc.result ?? '').split('\n').find(l => l.trim()) ?? ''
                        return line.trim().slice(0, 120)
                      })
                      out.push(
                        <RetryChainBanner
                          key={`chain-${tc.tool_use_id}`}
                          count={chainLen}
                          reasons={reasons}
                        />,
                      )
                      for (let k = i; k <= j; k++) out.push(renderTool(tcs[k], tcs[k].tool_use_id))
                      i = j + 1
                      continue
                    }
                  }
                  out.push(renderTool(tc, tc.tool_use_id))
                  i++
                }
                return out
              })()}
            </div>
          )}
        </div>
        <ActionToolbar className="opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity">
          <Button aria-label={t('chat.message.copy.aria')} onClick={handleCopy} className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30">
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">content_copy</span>
          </Button>
          {!isTool && (
            <>
              <Button aria-label={t('chat.message.like.aria')} aria-pressed={myRating === 'up'} onClick={() => toggleFeedback('up')} className={cn('flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30', myRating === 'up' ? 'text-primary' : 'text-on-surface-variant')}>
                <span className="material-symbols-outlined text-[18px]" aria-hidden="true">{myRating === 'up' ? 'thumb_up' : 'thumb_up_off_alt'}</span>
              </Button>
              <Button aria-label={t('chat.message.dislike.aria')} aria-pressed={myRating === 'down'} onClick={() => toggleFeedback('down')} className={cn('flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30', myRating === 'down' ? 'text-error' : 'text-on-surface-variant')}>
                <span className="material-symbols-outlined text-[18px]" aria-hidden="true">{myRating === 'down' ? 'thumb_down' : 'thumb_down_off_alt'}</span>
              </Button>
              <Button aria-label={t('chat.message.regenerate.aria')} onClick={handleRegenerate} className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30">
                <span className="material-symbols-outlined text-[18px]" aria-hidden="true">refresh</span>
              </Button>
              <Button
                aria-label={t('chat.message.branch.aria')}
                onClick={handleBranch}
                disabled={isBranching || !currentSessionId}
                className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                title={t('chat.message.branch.button')}
              >
                <span className="material-symbols-outlined text-[18px]" aria-hidden="true">
                  {isBranching ? 'hourglass_empty' : 'fork_right'}
                </span>
              </Button>
            </>
          )}
          {hasReport && (
            <Button
              aria-label={t('chat.message.report.aria')}
              onClick={() => setReportOpen(true)}
              className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
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

/** P2-⑨: banner preceding a run of consecutive same-tool failures — links
 *  to the session's turn timeline where the retry narrative is visualized. */
function RetryChainBanner({ count, reasons = [] }: { count: number; reasons?: string[] }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const navigate = useNavigate()
  const { currentSessionId } = useSessions()
  return (
    <div className="px-md py-xs rounded-lg bg-error/5 border border-error/20" data-testid="retry-chain-banner">
      <div className="flex items-center gap-sm">
        <span className="material-symbols-outlined icon-sm text-error shrink-0" aria-hidden="true">replay</span>
        <span className="font-label-sm text-error flex-1 truncate">
          {intl.formatMessage({ id: 'chat.message.retryChain' }, { count })}
        </span>
        {currentSessionId && (
          <Button
            variant="ghost"
            size="sm"
            className="shrink-0 gap-xs px-sm py-xs text-on-surface-variant hover:text-primary"
            onClick={() => navigate(`/timeline/${currentSessionId}`)}
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">timeline</span>
            {t('chat.message.retryChain.view')}
          </Button>
        )}
      </div>
      {reasons.length > 0 && (
        <ul className="mt-xs space-y-[2px]">
          {reasons.map((line, idx) => (
            <li key={idx} className="flex items-start gap-xs font-label-xs text-on-surface-variant">
              <span className="font-mono text-on-surface-variant/70 shrink-0" aria-hidden="true">
                {intl.formatMessage({ id: 'chat.message.retryChain.attempt' }, { n: idx + 1 })}
              </span>
              <span className="truncate" title={line}>{line}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

/** P1-⑤ telemetry: compact wall-clock label — 842 ms · 5.2 s · 1m04s. */
export function formatToolDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`
  const s = ms / 1000
  if (s < 60) return `${s < 10 ? s.toFixed(1) : Math.round(s)} s`
  const m = Math.floor(s / 60)
  return `${m}m${String(Math.round(s % 60)).padStart(2, '0')}s`
}

export const ToolCallDisplay = memo(function ToolCallDisplay({ toolCall, onViewDiff, durationMs: durationMsProp }: { toolCall: ToolCall; onViewDiff: (path: string) => void; durationMs?: number }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  // P1-⑤: error cards default open — the failure text is the content the
  // user asked about; healthy calls stay collapsed (ZCode delta ⑤).
  const [expanded, setExpanded] = useState(toolCall.is_error === true)
  const statusIcon = toolCall.status === 'running' ? 'hourglass_empty' : toolCall.status === 'error' ? 'error' : 'check_circle'
  const statusColor = toolCall.status === 'running' ? 'text-secondary' : toolCall.status === 'error' ? 'text-error' : 'text-tertiary'
  const filePath = extractFilePath(toolCall.tool_name, toolCall.tool_input)
  const canDiff = filePath != null && toolCall.status === 'completed' && !toolCall.is_error
  const durationMs = toolCall.duration_ms ?? durationMsProp
  // P1-⑤: the engine's §4.12 metadata — surface a sandbox-denied verdict on
  // the card itself instead of burying it in the expanded JSON.
  const sandboxDenied = (() => {
    const meta = toolCall.meta
    if (!meta || typeof meta !== 'object') return false
    return (meta as Record<string, unknown>).classification === 'sandbox_denied'
  })()

  return (
    <Tool name={toolCall.tool_name} status={toolCall.status} className="p-sm">
      <ToolHeader onClick={() => setExpanded(!expanded)}>
        <span className={cn('material-symbols-outlined icon-sm', statusColor, toolCall.status === 'running' ? 'animate-spin' : '')}>{statusIcon}</span>
        <span className="font-label-md text-on-surface flex-1 truncate">{toolCall.tool_name}</span>
        {sandboxDenied && (
          <span
            role="img"
            aria-label={t('chat.tool.sandboxDenied')}
            title={t('chat.tool.sandboxDenied')}
            className="flex items-center gap-[2px] shrink-0 px-xs py-[1px] rounded bg-error/10 text-error font-label-xs"
          >
            <span className="material-symbols-outlined text-[12px]" aria-hidden="true">shield</span>
            {t('chat.tool.sandboxDenied')}
          </span>
        )}
        {toolCall.status !== 'running' && durationMs != null && (
          <span className="font-mono text-label-xs tabular-nums text-on-surface-variant/80 shrink-0" aria-hidden="true">
            {formatToolDuration(durationMs)}
          </span>
        )}
        {toolCall.status !== 'running' && (toolCall.tokens_used ?? 0) > 0 && (
          <span
            className="font-mono text-label-xs tabular-nums text-on-surface-variant/80 shrink-0"
            title={t('chat.tool.tokens.title')}
          >
            {toolCall.tokens_used!.toLocaleString()} tok
          </span>
        )}
        {canDiff && (
          <Button
            variant="ghost"
            size="sm"
            aria-label={intl.formatMessage({ id: 'chat.message.diff.aria' }, { path: filePath })}
            className="gap-xs px-xs py-[2px] text-tertiary hover:bg-tertiary-container/40"
            onClick={(e) => { e.stopPropagation(); onViewDiff(filePath!) }}
          >
            <span className="material-symbols-outlined icon-sm">difference</span>
            {t('chat.message.diff')}
          </Button>
        )}
        <span className="material-symbols-outlined icon-sm text-on-surface-variant" aria-hidden="true">{expanded ? 'expand_less' : 'expand_more'}</span>
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

/**
 * P1-⑥ (ZCode delta): first-class collapsible block for `agent_spawn` tool
 * calls — sub-agent runs render as their own timeline section (name, model,
 * team, spawn prompt + result summary) instead of a generic tool card.
 * Engine note: with agent teams enabled (B2), the registry bridges spawn
 * lifecycle to `subagent:start` / `subagent:stop` — while the spawn tool is
 * running, the block surfaces the live registry agent id via `subagentLive`.
 */
export const SubagentBlock = memo(function SubagentBlock({ toolCall }: { toolCall: ToolCall }) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values)
  const { subagentLive } = useSessions()
  const [expanded, setExpanded] = useState(false)
  const input = (toolCall.tool_input ?? {}) as Record<string, unknown>
  const name = typeof input.name === 'string' ? input.name : ''
  const model = typeof input.model === 'string' ? input.model : null
  const team = typeof input.team === 'string' ? input.team : null
  const maxTurns = typeof input.max_turns === 'number' ? input.max_turns : null
  const systemPrompt = typeof input.system_prompt === 'string' ? input.system_prompt : ''
  const statusIcon = toolCall.status === 'running' ? 'hourglass_empty' : toolCall.status === 'error' ? 'error' : 'check_circle'
  const statusColor = toolCall.status === 'running' ? 'text-secondary' : toolCall.status === 'error' ? 'text-error' : 'text-tertiary'
  const durationMs = toolCall.duration_ms

  return (
    <div
      className="rounded-xl border border-primary/20 bg-primary/5 overflow-hidden"
      data-testid="subagent-block"
    >
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        className="w-full flex items-center gap-sm px-sm py-xs text-left cursor-pointer hover:bg-primary/10 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
      >
        <span className="material-symbols-outlined icon-sm text-primary shrink-0" aria-hidden="true">account_tree</span>
        <span className="font-label-md text-on-surface flex-1 truncate">
          {t('chat.subagent.title', { name: name || t('chat.subagent.unnamed') })}
        </span>
        {model && (
          <span className="font-mono text-label-xs text-on-surface-variant px-xs py-[1px] rounded bg-surface-container shrink-0" aria-hidden="true">{model}</span>
        )}
        {team && (
          <span className="font-label-xs text-on-surface-variant px-xs py-[1px] rounded bg-surface-container shrink-0" aria-hidden="true">{team}</span>
        )}
        {toolCall.status === 'running' && subagentLive && (
          <span className="font-mono text-label-xs px-xs py-[1px] rounded bg-primary/10 text-primary shrink-0 flex items-center gap-1" aria-live="polite">
            <span className="size-1.5 rounded-full bg-primary animate-pulse" aria-hidden="true" />
            {t('chat.subagent.registryId', { id: subagentLive.agentId })}
          </span>
        )}
        {toolCall.status !== 'running' && durationMs != null && (
          <span className="font-mono text-label-xs tabular-nums text-on-surface-variant/80 shrink-0" aria-hidden="true">
            {formatToolDuration(durationMs)}
          </span>
        )}
        <span className={cn('material-symbols-outlined icon-sm shrink-0', statusColor, toolCall.status === 'running' ? 'animate-spin' : '')} aria-hidden="true">{statusIcon}</span>
        <span className="material-symbols-outlined icon-sm text-on-surface-variant" aria-hidden="true">{expanded ? 'expand_less' : 'expand_more'}</span>
      </button>
      {expanded && (
        <div className="px-sm pb-sm space-y-sm">
          {maxTurns != null && (
            <p className="font-label-sm text-on-surface-variant">{t('chat.subagent.maxTurns', { count: maxTurns })}</p>
          )}
          {systemPrompt && (
            <pre className="text-body-sm text-on-surface-variant bg-surface-container p-sm rounded-lg overflow-x-auto max-h-[160px] whitespace-pre-wrap">{systemPrompt}</pre>
          )}
          {toolCall.result && (
            <pre className={cn(
              'text-body-sm p-sm rounded-lg overflow-x-auto max-h-[200px]',
              toolCall.is_error ? 'bg-error/5 text-error' : 'bg-surface-container text-on-surface-variant',
            )}>{toolCall.result}</pre>
          )}
        </div>
      )}
    </div>
  )
})
