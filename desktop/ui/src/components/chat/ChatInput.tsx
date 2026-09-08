import { useState, useRef, useEffect, useLayoutEffect, useCallback } from 'react'
import { useIntl } from 'react-intl'
import { open } from '@tauri-apps/plugin-dialog'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useCatalog } from '@/context/CatalogContext'
import { useVoice } from '@/hooks/useVoice'
import { MicButton } from '@/components/voice/MicButton'
import { VoiceOrb } from '@/components/voice/VoiceOrb'
import AttachmentChip from '@/components/chat/AttachmentChip'
import { isSlashQuery, filterSlashCommands, type SlashCommand } from '@/lib/slash/commands'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'])

/* Char-count thresholds.
 *   showAt — start showing the live counter
 *   softWarnAt — visually promote (orange/yellow) without blocking
 * Beyond softWarn the counter is just a louder warning; the user can
 * still hit send. Hard limits should go through the Rust backend. */
const CHAR_SHOW_AT = 2000
const CHAR_SOFT_WARN_AT = 8000

interface ChatInputProps {
  value: string
  onChange: (value: string) => void
  onSend: () => void
  /** Runs a picked slash command (clears the input itself afterwards). */
  onExecuteSlash: (cmd: SlashCommand) => void
  attachedFiles: string[]
  onAttach: (files: string[]) => void
  onDetachAll: () => void
  disabled: boolean
  isQuerying: boolean
  onCancelQuery: () => void
  onOpenQuickFix: () => void
  onOpenEditor: () => void
}

// U2: the model Select and the working-directory chip were removed — the
// global Header owns model switching, and the composer footer (ComposerPanel)
// is the single working-directory entry point.
export default function ChatInput({
  value,
  onChange,
  onSend,
  onExecuteSlash,
  attachedFiles,
  onAttach,
  onDetachAll,
  disabled,
  isQuerying,
  onCancelQuery,
  onOpenQuickFix,
  onOpenEditor,
}: ChatInputProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { config, refreshConfig } = useCatalog()
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const [isDragging, setIsDragging] = useState(false)

  // Slash-command autocomplete: open while the input is a single `/token`.
  // Escape hides it until the query changes again; a space or newline closes
  // it naturally (the query regex stops matching), turning the text back
  // into a regular prompt.
  const [slashDismissed, setSlashDismissed] = useState(false)
  const [slashActive, setSlashActive] = useState(0)
  const slashQuery = isSlashQuery(value) && !isQuerying ? value.trim() : null
  const slashMatches = slashQuery && !slashDismissed ? filterSlashCommands(slashQuery) : []

  useEffect(() => {
    setSlashActive(0)
    if (!isSlashQuery(value)) setSlashDismissed(false)
  }, [value])

  const executeSlash = (cmd: SlashCommand) => {
    onChange('')
    setSlashDismissed(false)
    onExecuteSlash(cmd)
  }
  const voice = useVoice({
    onTranscript: (text) => {
      const merged = value ? `${value} ${text}` : text
      onChange(merged)
    },
    onError: (msg) => toastError(t('voice.error.title'), msg),
    // P2-5e: prefer the local provider when the user has enabled
    // it in Settings → Voice. The cloud provider is the fallback
    // (the default) so existing users see no change.
    provider: config?.voice_local?.enabled ? 'local' : 'cloud',
    local: config?.voice_local
      ? {
          model: config.voice_local.model,
          language: config.voice_local.language,
        }
      : undefined,
  })

  const handleModeChange = async (mode: string | null) => {
    if (!mode) return
    try {
      await api.configure({ key: 'approval_mode', value: mode })
      await refreshConfig()
    } catch (err) {
      toastError(t('chat.input.mode.failed'), err)
    }
  }

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(true)
  }

  const handleDragLeave = (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
  }

  const mergePaths = (paths: string[]) => {
    const merged = [...new Set([...attachedFiles, ...paths])]
    if (merged.length > api.MAX_ATTACHMENT_COUNT) {
      const tooMany = intl.formatMessage(
        { id: 'chat.input.attach.tooMany' },
        { max: api.MAX_ATTACHMENT_COUNT },
      )
      toastError(t('chat.input.attach.failed'), tooMany)
      return
    }
    onAttach(merged)
  }

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)

    const files: FileList = e.dataTransfer.files
    if (!files || files.length === 0) return

    const paths: string[] = []
    for (let i = 0; i < files.length; i++) {
      const file = files[i]
      if ('path' in file && typeof file.path === 'string') {
        paths.push(file.path)
      }
    }

    if (paths.length > 0) {
      mergePaths(paths)
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // Slash menu captures the navigation keys while it is open.
    if (slashMatches.length > 0) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        const delta = e.key === 'ArrowDown' ? 1 : -1
        setSlashActive(i => (i + delta + slashMatches.length) % slashMatches.length)
        return
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault()
        executeSlash(slashMatches[slashActive] ?? slashMatches[0])
        return
      }
      if (e.key === 'Escape') {
        e.preventDefault()
        setSlashDismissed(true)
        return
      }
    }
    // Enter -> send; Shift/Ctrl+Enter -> newline. Matches VS Code's
    // Ctrl+Enter convention; preserves the legacy Enter-to-send UX.
    if (e.key === 'Enter' && !e.shiftKey && !(e.ctrlKey || e.metaKey)) {
      e.preventDefault()
      onSend()
    } else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.shiftKey) {
      e.preventDefault()
      onSend()
    }
    if (e.key === 'Escape' && isQuerying) {
      e.preventDefault()
      onCancelQuery()
    }
  }

  const handleAttachClick = async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          { name: t('chat.input.attach.filter.images'), extensions: Array.from(IMAGE_EXTENSIONS) },
          { name: t('chat.input.attach.filter.all'), extensions: ['*'] },
        ],
      })
      if (!selected) return
      const paths = (Array.isArray(selected) ? selected : [selected]) as string[]
      if (paths.length > 0) mergePaths(paths)
    } catch (err) {
      toastError(t('chat.input.attach.failed'), err)
    }
  }

  const currentMode = config?.approval_mode || 'suggest'
  const planModeActive = currentMode === 'plan'

  const handlePlanToggle = async () => {
    try {
      await api.configure({ key: 'approval_mode', value: planModeActive ? 'suggest' : 'plan' })
      await refreshConfig()
    } catch (err) {
      toastError(t('chat.input.planMode.failed'), err)
    }
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'P' || e.key === 'p')) {
        e.preventDefault()
        void handlePlanToggle()
      }
      // `/` focuses the composer (when not already typing in an input)
      if (e.key === '/' && !isQuerying && document.activeElement?.tagName !== 'TEXTAREA') {
        e.preventDefault()
        textareaRef.current?.focus()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [planModeActive])

  /* Auto-resize — grows to ~6 lines, then scrolls. Resets to 1 row on
   * blank input. Done in layout effect so the DOM is updated before
   * the browser paints (no flash). */
  const autosizeTextarea = useCallback(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    const maxPx = 200
    const next = Math.min(el.scrollHeight, maxPx)
    el.style.height = `${Math.max(next, 24)}px`
  }, [])

  useLayoutEffect(() => {
    autosizeTextarea()
  }, [value, autosizeTextarea])

  const modeOptions = [
    { value: 'readonly', label: t('chat.input.mode.readonly'), icon: 'lock', color: 'border-green-500/50' },
    { value: 'plan', label: t('chat.input.mode.plan'), icon: 'description', color: 'border-green-500/50' },
    { value: 'suggest', label: t('chat.input.mode.suggest'), icon: 'shield', color: 'border-amber-500/50' },
    { value: 'auto', label: t('chat.input.mode.auto'), icon: 'flash_auto', color: 'border-amber-500/50' },
    { value: 'full_auto', label: t('chat.input.mode.full_auto'), icon: 'bolt', color: 'border-red-500/50' },
  ]

  const selectedMode = modeOptions.find(m => m.value === currentMode) || modeOptions[2]

  /* Char count UI */
  const charCount = value.length
  const showCharCount = charCount >= CHAR_SHOW_AT
  const isOverSoftWarn = charCount >= CHAR_SOFT_WARN_AT

  return (
    <div
      className={cn('relative group transition-all', isDragging ? 'ring-2 ring-primary/50 rounded-2xl' : '')}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      role="region"
      aria-label={t('chat.input.ariaLabel')}
    >
      {slashMatches.length > 0 && (
        <div
          role="listbox"
          aria-label={t('slash.menu.aria')}
          className="absolute left-0 right-0 bottom-full mb-sm z-modal rounded-2xl border border-outline-variant/30 bg-surface-container-low shadow-lg overflow-hidden"
        >
          <ul className="max-h-64 overflow-y-auto py-xs">
            {slashMatches.map((cmd, i) => (
              <li key={cmd.name}>
                <button
                  type="button"
                  role="option"
                  aria-selected={i === slashActive}
                  onMouseDown={e => { e.preventDefault(); executeSlash(cmd) }}
                  onMouseEnter={() => setSlashActive(i)}
                  className={cn(
                    'w-full flex items-center gap-sm px-md py-xs text-left cursor-pointer transition-colors',
                    i === slashActive ? 'bg-surface-container-high' : 'hover:bg-surface-container',
                  )}
                >
                  <span className="material-symbols-outlined icon-sm text-primary shrink-0">{cmd.icon}</span>
                  <span className="font-mono text-label-md text-on-surface shrink-0">/{cmd.name}</span>
                  <span className="font-label-sm text-on-surface-variant truncate flex-1">{t(cmd.descriptionKey)}</span>
                </button>
              </li>
            ))}
          </ul>
          <div className="px-md py-xs border-t border-outline-variant/20 text-label-xs text-on-surface-variant">
            {t('slash.menu.hint')}
          </div>
        </div>
      )}

      {isDragging && (
        <div className="absolute inset-0 z-raised flex items-center justify-center bg-primary/10 rounded-2xl backdrop-blur-sm pointer-events-none">
          <div className="flex flex-col items-center gap-sm text-primary">
            <span className="material-symbols-outlined icon-xl">cloud_upload</span>
            <p className="font-label-md">{t('chat.input.attach.dropHint')}</p>
          </div>
        </div>
      )}

      {planModeActive && (
        <div
          role="status"
          className="flex items-center gap-xs px-md py-xs bg-tertiary-container/60 border-b border-tertiary/30 rounded-t-2xl text-on-tertiary-container"
        >
          <span className="material-symbols-outlined icon-sm shrink-0">route</span>
          <span className="font-label-sm truncate flex-1">{t('chat.input.planMode.banner')}</span>
          <Button
            variant="ghost"
            size="icon-xs"
            onClick={handlePlanToggle}
            aria-label={t('chat.input.planMode.exit')}
            title={t('chat.input.planMode.exit')}
            className="rounded hover:bg-tertiary/20 shrink-0"
          >
            <span className="material-symbols-outlined icon-sm">close</span>
          </Button>
        </div>
      )}

      {voice.state !== 'idle' && (
        <div className="flex items-center justify-center py-sm bg-primary/5 rounded-t-2xl">
          <VoiceOrb state={voice.state} />
        </div>
      )}

      <div className="flex flex-col">
        {attachedFiles.length > 0 && (
          <div className="flex flex-wrap items-center gap-xs px-md pt-md">
            {attachedFiles.map((path, i) => (
              <AttachmentChip key={path} path={path} onRemove={() => onAttach(attachedFiles.filter((_, idx) => idx !== i))} />
            ))}
            {attachedFiles.length > 1 && (
              <Button variant="link" size="sm" className="text-xs h-auto px-0 text-on-surface-variant hover:text-error ml-xs" onClick={onDetachAll}>
                {t('chat.input.attach.detachAll')}
              </Button>
            )}
          </div>
        )}

        <div className="flex items-start px-sm">
          <span className="material-symbols-outlined p-md text-primary shrink-0">
            {isQuerying ? 'hourglass_empty' : 'auto_awesome'}
          </span>
          <textarea
            ref={textareaRef}
            className="flex-1 bg-transparent border-none outline-none focus:ring-0 font-body-lg py-md px-sm placeholder:text-on-surface-variant/70 text-on-surface resize-none min-h-[24px] max-h-[200px]"
            placeholder={isQuerying ? t('chat.input.processing') : t('chat.input.placeholder')}
            aria-label={t('chat.input.ariaLabel')}
            value={value}
            onChange={e => onChange(e.target.value)}
            onKeyDown={handleKeyDown}
            rows={1}
            disabled={disabled}
          />
        </div>

        <div className="flex items-center justify-between gap-xs px-sm py-xs border-t border-outline-variant/20">
          <div className="flex items-center gap-xs flex-wrap min-w-0">
            <Button
              variant="outline"
              size="sm"
              onClick={handlePlanToggle}
              aria-pressed={planModeActive}
              aria-label={t('chat.input.planMode.aria')}
              title={t('chat.input.planMode.tooltip')}
              className={cn('h-auto gap-xs px-sm py-xs rounded-full text-label-sm shrink-0',
                planModeActive
                  ? 'border-primary bg-primary/10 text-primary'
                  : 'border-outline-variant/30 bg-surface-container-lowest/60 text-on-surface-variant hover:bg-surface-container-low hover:border-outline-variant hover:text-primary'
              )}
            >
              <span className="material-symbols-outlined icon-sm">route</span>
              <span>{t('chat.input.planMode.label')}</span>
            </Button>

            <Select value={currentMode} onValueChange={handleModeChange}>
              <SelectTrigger
                size="sm"
                aria-label={t('chat.input.mode.label')}
                className={cn('border', selectedMode.color, 'bg-transparent hover:bg-surface-container-low/50 transition-colors')}
              >
                <span className="material-symbols-outlined icon-sm">{selectedMode.icon}</span>
                <SelectValue placeholder={t('chat.input.mode.label')} />
              </SelectTrigger>
              <SelectContent>
                {modeOptions.map(mode => (
                  <SelectItem key={mode.value} value={mode.value}>
                    <div className="flex items-center gap-xs">
                      <span className="material-symbols-outlined icon-sm">{mode.icon}</span>
                      <span>{mode.label}</span>
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex items-center gap-xs shrink-0">
            <Button
              variant="ghost"
              aria-label={t('chat.input.attach.aria')}
              title={t('chat.input.attach.aria')}
              className="p-md text-on-surface-variant hover:text-primary"
              onClick={handleAttachClick}
            >
              <span className="material-symbols-outlined icon-md">attach_file</span>
            </Button>

            <Button
              variant="ghost"
              aria-label={t('nav.quickFix')}
              title={t('nav.quickFix')}
              className="p-md text-on-surface-variant hover:text-primary"
              onClick={onOpenQuickFix}
            >
              <span className="material-symbols-outlined icon-md">build</span>
            </Button>

            <Button
              variant="ghost"
              aria-label={t('nav.editor')}
              title={t('nav.editor')}
              className="p-md text-on-surface-variant hover:text-primary"
              onClick={onOpenEditor}
            >
              <span className="material-symbols-outlined icon-md">code</span>
            </Button>

            <MicButton
              state={voice.state}
              disabled={disabled}
              onStart={() => void voice.startRecording()}
              onStop={() => void voice.stopRecording()}
            />

            {showCharCount && (
              <span
                role="status"
                aria-live="polite"
                className={cn('font-mono text-label-xs tabular-nums px-xs', isOverSoftWarn ? 'text-error' : 'text-on-surface-variant/70')}
              >
                {charCount.toLocaleString()}
              </span>
            )}

            {isQuerying ? (
              <Button
                aria-label={t('chat.input.stop.aria')}
                className="bg-error/80 text-on-error p-3 rounded-xl active:scale-95 transition-all"
                onClick={onCancelQuery}
              >
                <span className="material-symbols-outlined icon-md">stop</span>
              </Button>
            ) : (
              <Button
                aria-label={t('chat.input.send.aria')}
                className="bg-primary text-on-primary p-3 rounded-xl active:scale-95 hover:shadow-md hover:shadow-primary/30 transition-all disabled:opacity-40 disabled:cursor-not-allowed"
                onClick={onSend}
                disabled={!value.trim() && attachedFiles.length === 0}
              >
                <span className="material-symbols-outlined icon-md">arrow_upward</span>
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
