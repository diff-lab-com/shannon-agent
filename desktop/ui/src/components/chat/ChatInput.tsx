import { useState, useRef, useEffect, useLayoutEffect, useId, useCallback } from 'react'
import { useIntl } from 'react-intl'
import { open } from '@tauri-apps/plugin-dialog'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'
import { useCatalog } from '@/context/CatalogContext'
import { useVoice } from '@/hooks/useVoice'
import { MicButton } from '@/components/voice/MicButton'
import { VoiceOrb } from '@/components/voice/VoiceOrb'
import AttachmentChip from '@/components/chat/AttachmentChip'
import SessionUsageDialog from '@/components/chat/SessionUsageDialog'
import { isSlashQuery, filterSlashCommands, type SlashCommand } from '@/lib/slash/commands'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'])

/** Last path segment, no extension — used by the contextual placeholder so a
 *  repo named "shannon-desktop" reads as "Working in shannon-desktop …". */
function basename(p: string): string {
  const trimmed = p.replace(/\\/g, '/').replace(/\/+$/, '')
  const last = trimmed.split('/').pop() ?? ''
  return last || trimmed
}

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
  /** Session working directory — picks a context-aware composer placeholder. */
  sessionWorkingDir?: string
  /** 最近一次流式 Usage payload — composer 侧会话用量弹框跟随刷新。 */
  usageTick?: unknown
}

// U2 removed the composer's model Select; the ZCode delta P0-③ brings a
// model chip back (per-message switching without reaching for the Header)
// while the global Header selector stays in sync — both write the same
// engine config keys (`model` holds a model NAME, not the catalog id).
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
  sessionWorkingDir,
  usageTick,
}: ChatInputProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { config, status, models, refreshConfig, refreshStatus } = useCatalog()
  // Tests (and degraded catalogs) may omit the model list — the chip then
  // falls back to the placeholder and lists nothing.
  const modelList = models ?? []
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const [isDragging, setIsDragging] = useState(false)

  // P2-9: combobox wiring — the textarea acts as the combobox and points at
  // the slash listbox via aria-controls/aria-activedescendant.
  const slashListboxId = useId()
  const slashOptionId = (name: string) => `${slashListboxId}-opt-${name}`

  // Slash-command autocomplete: open while the input is a single `/token`.
  // Escape hides it until the query changes again; a space or newline closes
  // it naturally (the query regex stops matching), turning the text back
  // into a regular prompt.
  const [slashDismissed, setSlashDismissed] = useState(false)
  const [slashActive, setSlashActive] = useState(0)
  const slashQuery = isSlashQuery(value) && !isQuerying ? value.trim() : null
  const slashMatches = slashQuery && !slashDismissed ? filterSlashCommands(slashQuery) : []
  const slashOpen = slashMatches.length > 0

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

  // P0-③ (ZCode delta): composer model chip. Mirrors Header.handleModelSwitch
  // exactly — configure the model NAME plus its provider, then refresh both
  // config and status so the two selectors stay in sync.
  const currentModel = modelList.find(m => m.name === status?.model || m.id === status?.model)
  const handleModelSwitch = async (modelId: string | null) => {
    const model = modelList.find(m => m.id === modelId)
    if (!model) return
    try {
      await api.configure({ key: 'model', value: model.name })
      await api.configure({ key: 'provider', value: model.provider })
      await refreshConfig()
      await refreshStatus()
    } catch (err) {
      toastError(t('chat.input.model.failed'), err)
    }
  }

  // Audit D8 — reasoning-effort picker. The engine already persists
  // `effort_level` (CLI /effort → config) and maps it to the provider's
  // reasoning parameter; the desktop composer previously had no surface for it.
  const currentEffort = (config as Record<string, unknown> | undefined)?.effort_level as string | undefined ?? 'medium'
  const effortOptions = [
    { value: 'low', label: t('chat.input.effort.low') },
    { value: 'medium', label: t('chat.input.effort.medium') },
    { value: 'high', label: t('chat.input.effort.high') },
    { value: 'max', label: t('chat.input.effort.max') },
  ]
  const handleEffortChange = async (effort: string | null) => {
    if (!effort) return
    try {
      await api.configure({ key: 'effort_level', value: effort })
      await refreshConfig()
    } catch (err) {
      toastError(t('chat.input.effort.failed'), err)
    }
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
  // B0 P0-2: the drag-drop subscription outlives single renders, so it
  // dispatches through a latest-ref instead of re-subscribing on every
  // attachments change.
  const mergePathsRef = useRef(mergePaths)
  useEffect(() => {
    mergePathsRef.current = mergePaths
  })

  // B0 P0-2 — file drag-drop via the webview's own Tauri v2 events. With
  // `dragDropEnabled` (the default) HTML5 dragover/drop never fire and
  // `File.path` no longer exists, so the overlay + attachment list are
  // driven entirely by onDragDropEvent (enter/over → show, out/leave →
  // hide, drop → attach the real absolute paths). In mock/demo mode the
  // registration resolves and nothing fires — acceptable.
  useEffect(() => {
    let unlisten: (() => void) | null = null
    let cancelled = false
    void api.onWebviewFileDrop(event => {
      if (event.type === 'enter' || event.type === 'over') {
        setIsDragging(true)
      } else if (event.type === 'leave') {
        setIsDragging(false)
      } else {
        // drop — attach the real absolute paths.
        setIsDragging(false)
        if (event.paths.length > 0) mergePathsRef.current(event.paths)
      }
    }).then(fn => {
      if (cancelled) fn?.()
      else unlisten = fn ?? null
    }).catch(() => {
      // Outside a Tauri webview (plain-browser dev) registration fails —
      // nothing fires, which is the acceptable mock/demo behavior.
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // B0 P0-3 — IME composition guard. While a CJK conversion is in flight,
  // the Enter/Tab keydown belongs to the IME (it confirms the candidate);
  // sending it would post half-converted pinyin. Two browser orderings need
  // covering:
  //   - Chrome fires keydown(isComposing=true) BEFORE compositionend —
  //     caught by the ref and by nativeEvent.isComposing;
  //   - Safari/Firefox fire compositionend BEFORE the confirming keydown,
  //     so that keydown arrives with every flag already false — caught by
  //     the just-ended timestamp window.
  const COMPOSITION_END_GRACE_MS = 100
  const isComposingRef = useRef(false)
  const compositionEndedAtRef = useRef(0)
  const isCompositionKey = (e: React.KeyboardEvent): boolean =>
    isComposingRef.current ||
    (e.nativeEvent as KeyboardEvent).isComposing ||
    Date.now() - compositionEndedAtRef.current < COMPOSITION_END_GRACE_MS

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // IME guard first: swallow only the send/execute keys so the candidate
    // window keeps them; navigation keys pass through untouched.
    if (isCompositionKey(e)) {
      if (e.key === 'Enter' || e.key === 'Tab') e.preventDefault()
      return
    }
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

  // The composer owns ONE mode surface (the unified pill below); the
  // keyboard shortcut and the plan banner's exit button both funnel here.
  // (The old design had a separate 计划模式 toggle button alongside the
  // approval-mode select that also contained 计划/只读 — two controls
  // writing the same `approval_mode` key, which read as overlapping modes.)
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
      // `/` focuses the composer — unless the keystroke already sits inside
      // any editable surface (search boxes, command palette, selects,
      // contentEditable), which the old TEXTAREA-only check let be hijacked.
      // Same guard shape as hooks/useKeyboardShortcuts.ts.
      if (e.key === '/' && !isQuerying) {
        const el = e.target as HTMLElement | null
        const inEditable =
          el != null &&
          (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable)
        if (!inEditable) {
          e.preventDefault()
          textareaRef.current?.focus()
        }
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
    { value: 'readonly', label: t('chat.input.mode.readonly'), desc: t('chat.input.mode.readonly.desc'), icon: 'lock', color: 'border-success/50' },
    { value: 'plan', label: t('chat.input.mode.plan'), desc: t('chat.input.mode.plan.desc'), icon: 'description', color: 'border-success/50' },
    { value: 'suggest', label: t('chat.input.mode.suggest'), desc: t('chat.input.mode.suggest.desc'), icon: 'shield', color: 'border-warning/50' },
    { value: 'auto', label: t('chat.input.mode.auto'), desc: t('chat.input.mode.auto.desc'), icon: 'flash_auto', color: 'border-warning/50' },
    { value: 'full_auto', label: t('chat.input.mode.full_auto'), desc: t('chat.input.mode.full_auto.desc'), icon: 'bolt', color: 'border-error/50' },
  ]

  const selectedMode = modeOptions.find(m => m.value === currentMode) || modeOptions[2]

  /* "+" menu — attachments and the two inline tools, one click each. */
  const [plusOpen, setPlusOpen] = useState(false)

  // SessionUsageDialog — composer 模型 chip 旁的会话用量入口(2026-09
  // 三项 UX 修复 #3)。弹框点击后才挂载,首屏零开销;打开期间跟随父
  // 组件透传的 streaming usageTick 实时刷新 breakdown。
  const [usageOpen, setUsageOpen] = useState(false)
  const plusItems: DropdownMenuItem[] = [
    { id: 'attach', label: t('chat.input.attach.aria'), icon: 'attach_file', onSelect: () => { setPlusOpen(false); void handleAttachClick() } },
    { id: 'quickfix', label: t('nav.quickFix'), icon: 'build', onSelect: () => { setPlusOpen(false); onOpenQuickFix() } },
    { id: 'editor', label: t('nav.editor'), icon: 'code', onSelect: () => { setPlusOpen(false); onOpenEditor() } },
  ]

  /* Char count UI */
  const charCount = value.length
  const showCharCount = charCount >= CHAR_SHOW_AT
  const isOverSoftWarn = charCount >= CHAR_SOFT_WARN_AT

  return (
    <div
      className={cn('relative group transition-all', isDragging ? 'ring-2 ring-primary/50 rounded-2xl' : '')}
      role="region"
      aria-label={t('chat.input.ariaLabel')}
    >
      {slashMatches.length > 0 && (
        <div
          id={slashListboxId}
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
                  id={slashOptionId(cmd.name)}
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
            placeholder={
              isQuerying
                ? t('chat.input.processing')
                : sessionWorkingDir
                  ? intl.formatMessage({ id: 'chat.input.placeholder.project' }, { dir: basename(sessionWorkingDir) })
                  : t('chat.input.placeholder.empty')
            }
            aria-label={t('chat.input.ariaLabel')}
            // P2-9: combobox a11y for the slash autocomplete — the listbox
            // exists only while the menu is open; selection is reflected via
            // aria-activedescendant pointing at the highlighted option.
            role="combobox"
            aria-expanded={slashOpen}
            aria-controls={slashOpen ? slashListboxId : undefined}
            aria-activedescendant={slashOpen ? slashOptionId(slashMatches[slashActive]?.name ?? '') : undefined}
            aria-autocomplete="list"
            value={value}
            onChange={e => onChange(e.target.value)}
            onKeyDown={handleKeyDown}
            onCompositionStart={() => {
              isComposingRef.current = true
              compositionEndedAtRef.current = 0
            }}
            onCompositionEnd={() => {
              isComposingRef.current = false
              // Stamps the grace window that covers the Safari/Firefox
              // ordering, where the confirming keydown lands afterwards.
              compositionEndedAtRef.current = Date.now()
            }}
            rows={1}
            disabled={disabled}
          />
        </div>

        <div className="flex items-center justify-between gap-xs px-sm py-xs border-t border-outline-variant/20">
          {/* Classic AI-composer layout: one "+" menu, ONE mode surface, the
              model pill (reasoning effort folded into its dropdown) on the
              left; mic / counter / send on the right. The old row showed a
              separate 计划模式 toggle next to the approval-mode select that
              also held 计划/只读 — two controls for the same key. */}
          <div className="flex items-center gap-xs flex-wrap min-w-0">
            <span className="relative shrink-0">
              <Button
                variant="ghost"
                aria-haspopup="menu"
                aria-expanded={plusOpen}
                aria-label={t('chat.input.plus.aria')}
                title={t('chat.input.plus.aria')}
                className="p-md text-on-surface-variant hover:text-primary"
                onClick={() => setPlusOpen(v => !v)}
              >
                <span className="material-symbols-outlined icon-md">add_circle_outline</span>
              </Button>
              {plusOpen && (
                <DropdownMenu
                  open
                  onClose={() => setPlusOpen(false)}
                  items={plusItems}
                  align="start"
                  className="w-48 min-w-0"
                  ariaLabel={t('chat.input.plus.aria')}
                />
              )}
            </span>

            <Select value={currentMode} onValueChange={handleModeChange}>
              <SelectTrigger
                size="sm"
                aria-label={t('chat.input.mode.label')}
                title={selectedMode.desc}
                className={cn('rounded-full border', selectedMode.color, 'bg-transparent hover:bg-surface-container-low/50 transition-colors')}
              >
                <span className="material-symbols-outlined icon-sm">{selectedMode.icon}</span>
                {/* Render the matched mode's local label, not the raw value
                    — an unknown approval_mode (e.g. legacy 'standard') now
                    falls back to the Suggest label instead of bleeding into
                    the pill chrome. */}
                <SelectValue placeholder={t('chat.input.mode.label')}>
                  {() => <span className="truncate">{selectedMode.label}</span>}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                {modeOptions.map(mode => (
                  <SelectItem key={mode.value} value={mode.value}>
                    <div className="flex items-start gap-xs py-0.5">
                      <span className="material-symbols-outlined icon-sm mt-0.5" aria-hidden="true">{mode.icon}</span>
                      <span className="min-w-0">
                        <span className="block font-label-md text-on-surface whitespace-nowrap">{mode.label}</span>
                        <span className="block font-label-xs text-on-surface-variant whitespace-normal">{mode.desc}</span>
                      </span>
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {/* 会话用量入口 — 模型 chip 旁一次点击(2026-09 三项 UX 修复 #3)。
                弹框点击后才挂载,composer 首屏零开销。 */}
            <Button
              variant="ghost"
              aria-haspopup="dialog"
              aria-expanded={usageOpen}
              aria-label={t('chat.input.usage.aria')}
              title={t('chat.input.usage.title')}
              className="p-md text-on-surface-variant hover:text-primary shrink-0"
              onClick={() => setUsageOpen(true)}
            >
              <span className="material-symbols-outlined icon-md" aria-hidden="true">data_usage</span>
            </Button>

            <Select
              value={currentModel?.id ?? ''}
              onValueChange={value => {
                // Reasoning effort is folded into the model dropdown as a
                // namespaced section — one pill instead of two.
                if (value && value.startsWith('effort:')) {
                  void handleEffortChange(value.slice('effort:'.length))
                  return
                }
                if (value) void handleModelSwitch(value)
              }}
            >
              <SelectTrigger
                size="sm"
                aria-label={t('chat.input.model.label')}
                title={t('chat.input.model.title')}
                className="max-w-[170px] rounded-full border border-outline-variant/50 bg-transparent hover:bg-surface-container-low/50 transition-colors"
              >
                <span className="material-symbols-outlined icon-sm">smart_toy</span>
                {/* Render the model NAME (what config `model` stores and what
                    the Header displays), not the catalog id. */}
                <SelectValue placeholder={status?.model || t('chat.input.model.label')}>
                  {(value: unknown) => {
                    // Reflect a non-default reasoning effort on the chip —
                    // the Select's value is always a model id (effort picks
                    // commit via `effort:` in onValueChange but never become
                    // the Select value), so read it from config directly.
                    const eff = effortOptions.find(e => e.value === currentEffort)
                    const m = modelList.find(x => x.id === value)
                    const name = m?.name ?? status?.model ?? t('chat.input.model.label')
                    if (eff && eff.value !== 'medium') {
                      return `${name} · ${eff.label}`
                    }
                    return name
                  }}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                {modelList.map(m => (
                  <SelectItem key={m.id} value={m.id}>
                    <span className="font-mono">{m.name}</span>
                  </SelectItem>
                ))}
                {modelList.length > 0 && (
                  <div role="presentation" className="mx-sm my-xs border-t border-outline-variant/20" />
                )}
                <div role="presentation" className="px-sm pt-0 pb-1 font-label-xs uppercase tracking-wider text-on-surface-variant">
                  {t('chat.input.effort.section')}
                </div>
                {effortOptions.map(effort => (
                  <SelectItem
                    key={`effort-${effort.value}`}
                    value={`effort:${effort.value}`}
                  >
                    <span className="flex items-center gap-xs">
                      <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                        {currentEffort === effort.value ? 'radio_button_checked' : 'radio_button_unchecked'}
                      </span>
                      {effort.label}
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex items-center gap-xs shrink-0">
            <MicButton
              state={voice.state}
              disabled={disabled}
              onStart={() => void voice.startRecording()}
              onStop={() => void voice.stopRecording()}
            />

            {showCharCount && (
              <>
                {/* P2-9: the counter used to carry aria-live="polite", which
                    announced every keystroke past 2000 chars. It is a purely
                    visual readout now; a static sr-only note about limits
                    replaces the per-key announcements. */}
                <span
                  className={cn('font-mono text-label-xs tabular-nums px-xs', isOverSoftWarn ? 'text-error' : 'text-on-surface-variant/70')}
                >
                  {charCount.toLocaleString()}
                </span>
                <span className="sr-only">{t('chat.input.charCount.hint')}</span>
              </>
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
      {/* SessionUsageDialog — 点击入口后才挂载(闭合成会话用量弹框);父组件
          透传 streaming usageTick,弹框打开期间随 token 流刷新。 */}
      {usageOpen && (
        <SessionUsageDialog open onClose={() => setUsageOpen(false)} usageTick={usageTick} />
      )}
    </div>
  )
}
