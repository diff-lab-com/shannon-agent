import { useState, useRef, useEffect, useLayoutEffect, useId, useCallback, useMemo } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { open } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'
import { useCatalog } from '@/context/CatalogContext'
import { useVoice } from '@/hooks/useVoice'
import { MicButton } from '@/components/voice/MicButton'
import { VoiceOrb } from '@/components/voice/VoiceOrb'
import AttachmentChip from '@/components/chat/AttachmentChip'
import SessionUsageDialog from '@/components/chat/SessionUsageDialog'
import PptOutlineDialog from '@/components/chat/PptOutlineDialog'
import { useComposerDraftListener } from '@/lib/composerBridge'
import { loadInputHistory } from '@/lib/inputHistory'
import { isSlashQuery, type SlashCommand } from '@/lib/slash/commands'
import { fetchSlashSkills, mergeSlashMenu, type SlashMenuItem, type SlashSkillEntry } from '@/lib/slash/skills'
import {
  activeMentionQuery,
  caretAfterMentionInsert,
  filterMentionCandidates,
  flattenFileTree,
  insertMention,
  relativeToWorkingDir,
} from '@/lib/fileMention'
import { imageFilesFromClipboard, blobToBase64, PASTE_IMAGE_MIME_TO_EXT, MAX_PASTED_IMAGE_BYTES } from '@/lib/pasteImage'
import * as api from '@/lib/tauri-api'
import type { RejectedAttachmentReason, AttachmentExtractionReport } from '@/types'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'
import { modelPickerMeta } from '@/components/settings/models-settings/types'
import { APPROVAL_MODES, approvalModeOption } from '@/lib/approvalModes'

/**
 * R2-P1-2 attachment honesty — exactly the image formats the backend's
 * multimodal whitelist turns into image blocks (png/jpeg/gif/webp; see
 * `is_vision_image_mime` in commands.rs). bmp/svg used to sit here too, so
 * the picker advertised formats the model never sees: the file attached,
 * the backend dropped it from the image blocks with no rejected receipt.
 * The drag-drop and paste paths bypass this filter by design — the backend
 * gate + preflight badge (`unsupported_type`) catch those.
 */
export const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp'])

/**
 * Office Wave 1 A1a, narrowed by G3 P1-5 — extensions whose content the
 * attachment pipeline does NOT parse. The backend extracts text for
 * docx/pptx/xlsx/ods/csv (document_parse.rs OFFICE_EXTENSIONS) and injects
 * it into the query, so those formats must NOT be listed here — the banner
 * used to claim the opposite of what the backend does on the formats users
 * attach most. What remains are the legacy binary/OTF formats with no
 * parser: the chip still attaches and the path is still sent, but the
 * composer says out loud that the file's CONTENT never reaches the model.
 * Keep in sync with document_parse::OFFICE_EXTENSIONS.
 */
export const UNPARSED_EXTENSIONS = new Set(['doc', 'xls', 'ppt', 'odt', 'rtf'])

/** Lowercased extension without the dot ('' for dotfiles/no extension). */
export function pathExtension(path: string): string {
  const name = path.replace(/\\/g, '/').split('/').pop() ?? ''
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : ''
}

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
  /** B1 P1-5/§4-9: the CURRENT session's query state. The textarea stays
   *  typable while streaming (queued sends); it only gates the mic, the
   *  stop/send swap and the Escape-cancels-run affordance. */
  isQuerying: boolean
  onCancelQuery: () => void
  /** S-3/A-18 companion (R4 group 7): a stop is already tearing the run
   *  down — the stop button renders a disabled "cancelling" state so a
   *  second press gets feedback instead of a silent backend no-op. */
  cancelInFlight?: boolean
  /** GB P2-10a: the interrupt-now send (bolt button / Ctrl+Enter while
   *  streaming). Enter keeps queueing; absent → Enter/Ctrl+Enter both send. */
  onSteer?: () => void
  /** B1 §4-8: present only while a message edit is in flight — Escape
   *  cancels the edit (restores the pre-edit draft) instead. */
  onCancelEdit?: () => void
  onOpenQuickFix: () => void
  onOpenEditor: () => void
  /** Session working directory — picks a context-aware composer placeholder. */
  sessionWorkingDir?: string
  /** 最近一次流式 Usage payload — composer 侧会话用量弹框跟随刷新。 */
  usageTick?: unknown
  /** R2-1: the session this composer targets. When present, model-chip
   *  switches are SESSION-scoped (`set_session_model`) and the chip shows a
   *  "· session" suffix while an override is active; "Set as default" in the
   *  chip menu performs the global configure. Omitted → legacy global
   *  behavior (no session context to scope to). */
  sessionId?: string | null
}

// U2 removed the composer's model Select; the ZCode delta P0-③ brings a
// model chip back (per-message switching without reaching for the Header).
// R2-1 splits the two intents: a chip switch now re-targets only the CURRENT
// session, while the menu's "Set as default" action writes the global
// config — the Header selector keeps showing that global default.
import { promoteSessionModelToDefault } from './sessionModelPromotion'

export default function ChatInput({
  value,
  onChange,
  onSend,
  onExecuteSlash,
  attachedFiles,
  onAttach,
  onDetachAll,
  isQuerying,
  onCancelQuery,
  cancelInFlight,
  onCancelEdit,
  onOpenQuickFix,
  onOpenEditor,
  onSteer,
  sessionWorkingDir,
  usageTick,
  sessionId,
}: ChatInputProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const navigate = useNavigate()
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
  // GB P2-10b: the @-mention listbox gets its own namespace of ids.
  const mentionListboxId = useId()

  // Slash-command autocomplete: open while the input is a single `/token`.
  // Escape hides it until the query changes again; a space or newline closes
  // it naturally (the query regex stops matching), turning the text back
  // into a regular prompt. B1 §4-9: also usable while streaming — local
  // slash commands never need to queue.
  const [slashDismissed, setSlashDismissed] = useState(false)
  const [slashActive, setSlashActive] = useState(0)
  // G1 P0-2.2 — installed skills join the menu as slash-triggered entries.
  // Fetched once on mount; error/timeout degrades to [] (static table only).
  const [skills, setSkills] = useState<SlashSkillEntry[]>([])
  useEffect(() => {
    let cancelled = false
    fetchSlashSkills().then(entries => {
      if (!cancelled) setSkills(entries)
    }).catch(() => {
      // fetchSlashSkills already swallows; this is belt-and-braces.
      if (!cancelled) setSkills([])
    })
    return () => { cancelled = true }
  }, [])
  const slashQuery = isSlashQuery(value) ? value.trim() : null
  const slashItems: SlashMenuItem[] =
    slashQuery && !slashDismissed ? mergeSlashMenu(skills, slashQuery) : []
  const slashMatches = slashItems
  const slashOpen = slashMatches.length > 0

  // GB P2-10b — @ file reference. The candidate universe loads once per
  // working dir: the working-dir tree (backend-bounded walk) plus the
  // session file index (attachments/favorites). Both fail soft — the menu
  // just stays empty, never breaks the composer.
  const [mentionCandidates, setMentionCandidates] = useState<string[]>([])
  useEffect(() => {
    let cancelled = false
    const tree: Promise<string[]> = sessionWorkingDir
      ? api.getFileTree(sessionWorkingDir).then(nodes => flattenFileTree(nodes)).catch(() => [])
      : Promise.resolve([])
    const index: Promise<string[]> = api.listFileIndex()
      .then(rows => rows.map(r => r.path).filter((p): p is string => typeof p === 'string'))
      .catch(() => [])
    Promise.all([tree, index]).then(([treePaths, indexPaths]) => {
      if (cancelled) return
      setMentionCandidates([...new Set([...treePaths, ...indexPaths])])
    })
    return () => { cancelled = true }
  }, [sessionWorkingDir])

  // The live `@query` — detected in the text BEFORE the caret (tracked on
  // every change/keyup/select so a mid-text mention works). Slash queries
  // and mentions are mutually exclusive by construction (`/^\/token$/` vs
  // whitespace-anchored `@token`).
  const caretRef = useRef(value.length)
  const mentionQuery = useMemo(
    () => activeMentionQuery(value.slice(0, caretRef.current)),
    [value],
  )
  const [mentionDismissed, setMentionDismissed] = useState(false)
  const [mentionActive, setMentionActive] = useState(0)
  const mentionMatches = mentionQuery && !mentionDismissed
    ? filterMentionCandidates(mentionCandidates, mentionQuery.token)
    : []
  const trackCaret = (el: HTMLTextAreaElement) => {
    caretRef.current = el.selectionStart ?? el.value.length
  }

  // Office Wave 1 A1a — honest notice while an unparseable attachment
  // (.doc/.xls/… legacy format) rides along. Dismissible, but re-arming:
  // once every unsupported file is removed the dismissal resets, so a NEW
  // .doc warns again instead of relying on a stale dismiss.
  const hasUnparsedAttachment = attachedFiles.some(p => UNPARSED_EXTENSIONS.has(pathExtension(p)))
  const [unparsedDismissed, setUnparsedDismissed] = useState(false)
  useEffect(() => {
    if (!hasUnparsedAttachment) setUnparsedDismissed(false)
  }, [hasUnparsedAttachment])
  const showUnparsedNotice = hasUnparsedAttachment && !unparsedDismissed

  // P0-3 preflight — at attach time, ask the backend how `send_message`
  // would treat each path, and flag the chip in place (warning icon +
  // tooltip) instead of letting the file silently vanish on send. Advisory:
  // a failed preflight call just means no marking.
  const [pathIssues, setPathIssues] = useState<Record<string, RejectedAttachmentReason>>({})
  // G3b P1-4 — extraction summaries the preflight returned for parseable
  // attachments (docx/pdf/…): rendered as chip badges so "extracted N
  // sections / PDF truncates at 50 KiB" is visible BEFORE the send. Same
  // lifecycle as the issue flags: advisory and pruned with the chips.
  const [pathReports, setPathReports] = useState<Record<string, AttachmentExtractionReport>>({})
  // R7-③ threshold hybrid — paths the preflight deferred (large parseable
  // documents): the chip shows the "parsed on send" placeholder instead of
  // an extraction badge; the real badge lights from the send receipt. Same
  // advisory lifecycle as the flags above.
  const [deferredParsePaths, setDeferredParsePaths] = useState<Record<string, boolean>>({})
  useEffect(() => {
    // Prune issues for chips the parent removed (detach-all, edit restore).
    setPathIssues(prev => {
      const alive = new Set(attachedFiles)
      const next: typeof prev = {}
      let changed = false
      for (const [p, reason] of Object.entries(prev)) {
        if (alive.has(p)) next[p] = reason
        else changed = true
      }
      return changed ? next : prev
    })
    setPathReports(prev => {
      const alive = new Set(attachedFiles)
      const next: typeof prev = {}
      let changed = false
      for (const [p, report] of Object.entries(prev)) {
        if (alive.has(p)) next[p] = report
        else changed = true
      }
      return changed ? next : prev
    })
    setDeferredParsePaths(prev => {
      const alive = new Set(attachedFiles)
      const next: typeof prev = {}
      let changed = false
      for (const p of Object.keys(prev)) {
        if (alive.has(p)) next[p] = prev[p]
        else changed = true
      }
      return changed ? next : prev
    })
  }, [attachedFiles])
  const hasNoWorkingDirIssue = Object.values(pathIssues).includes('no_working_dir')

  useEffect(() => {
    setSlashActive(0)
    setMentionActive(0)
    if (!isSlashQuery(value)) setSlashDismissed(false)
    // Mention dismissal re-arms when the query dissolves (whitespace after
    // the token) — same protocol as the slash menu: Escape holds the menu
    // closed for the query it dismissed, a NEW @query opens fresh.
    if (!activeMentionQuery(value.slice(0, caretRef.current))) setMentionDismissed(false)
    // External value writes (draft push, skill/slash fill) move the caret to
    // the end — keep the mention detector's caret from going stale.
    if (document.activeElement !== textareaRef.current) caretRef.current = value.length
  }, [value])

  /** GB P2-10b: replace the live `@query` with the picked path (plain text,
   *  TUI-style reference — no attachment semantics). */
  const commitMention = (path: string) => {
    if (!mentionQuery) return
    const next = insertMention(value, mentionQuery, path, sessionWorkingDir)
    const caret = caretAfterMentionInsert(mentionQuery, path, sessionWorkingDir)
    caretRef.current = caret
    onChange(next)
    // Dismiss for THIS query; re-arms automatically once the text stops
    // matching (same protocol as the slash menu).
    setMentionDismissed(true)
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (!el) return
      el.focus()
      el.setSelectionRange(caret, caret)
    })
  }

  const executeSlash = (cmd: SlashCommand) => {
    onChange('')
    setSlashDismissed(false)
    onExecuteSlash(cmd)
  }
  // G1 P0-2.2 — selecting a skill fills the composer with its slash trigger
  // (REPL-style `/name` semantics) so the user can add args and send; the
  // message reaches the model, which can invoke the registered skill tool.
  const executeSkill = (skill: SlashSkillEntry) => {
    onChange(`${skill.trigger} `)
    // Keep the menu closed while the text is still a bare `/name` query —
    // dismissal re-arms automatically once the input stops matching.
    setSlashDismissed(true)
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

  // P0-③ (ZCode delta): composer model chip. R2-1 — with a session context
  // the switch is session-scoped (`set_session_model`); without one the
  // legacy global configure applies. "Set as default" always goes global.
  // R2-1: the session's model override — mirrors the backend's
  // `SessionState.model_override`. Re-read whenever the focused session
  // changes so the chip never shows a stale override after a switch.
  // Presence of the prop (not truthiness) gates session-scoping: `null`
  // means "no focused id — the backend resolves the ACTIVE session", which
  // is exactly what a brand-new chat is.
  const sessionScoped = sessionId !== undefined
  const [sessionOverride, setSessionOverride] = useState<api.SessionModelOverride | null>(null)
  useEffect(() => {
    let cancelled = false
    if (!sessionScoped) {
      setSessionOverride(null)
      return
    }
    api.getSessionModel(sessionId ?? null)
      .then(ov => { if (!cancelled) setSessionOverride(ov ?? null) })
      .catch(() => { if (!cancelled) setSessionOverride(null) })
    return () => { cancelled = true }
  }, [sessionScoped, sessionId])

  // R2-1: the chip reflects the SESSION override when one is active — the
  // global `status` stays untouched so the Header keeps showing the default.
  const currentModel =
    (sessionOverride
      ? modelList.find(m => m.id === sessionOverride.model || m.name === sessionOverride.model)
      : undefined) ??
    modelList.find(m => m.name === status?.model || m.id === status?.model)
  const handleModelSwitch = async (modelId: string | null) => {
    const model = modelList.find(m => m.id === modelId)
    if (!model) return
    try {
      if (sessionScoped) {
        // R2-1: chip switch is session-scoped. Writes the canonical catalog
        // id (same normalization contract as `configure('model')`); a null
        // session id resolves to the active session backend-side.
        await api.setSessionModel(sessionId ?? null, model.provider, model.id)
        setSessionOverride({ provider: model.provider, model: model.id })
      } else {
        // No session context (tests / degraded catalogs) — legacy global write.
        await api.configure({ key: 'model', value: model.id })
        await api.configure({ key: 'provider', value: model.provider })
        await refreshConfig()
        await refreshStatus()
      }
    } catch (err) {
      toastError(t('chat.input.model.failed'), err)
    }
  }

  // R2-1: promote the chip's CURRENT effective target (session override when
  // active, else the displayed global model) to the engine-global default —
  // exactly the configure('model') + configure('provider') pair the chip
  // performed before R2-1.
  const handleSetAsDefault = async () => {
    const target = currentModel
    if (!target) return
    try {
      await promoteSessionModelToDefault(target, {
        configure: api.configure,
        refreshConfig,
        refreshStatus,
      })
      toast.success(intl.formatMessage({ id: 'chat.input.model.setDefault.toast' }, { model: target.name }))
    } catch (err) {
      toastError(t('chat.input.model.setDefault.failed'), err)
    }
  }

  // R2-1: drop the session override — the session re-inherits the global
  // default (including future default changes).
  const handleClearSessionOverride = async () => {
    if (!sessionScoped) return
    try {
      await api.clearSessionModel(sessionId ?? null)
      setSessionOverride(null)
    } catch (err) {
      toastError(t('chat.input.model.resetSession.failed'), err)
    }
  }

  // P2-5 — session-level "temporary chat" toggle: while active, this
  // session's queries are built without the memory layer (no injection of
  // past memories, no auto-extraction). Mirrors the sessionOverride block:
  // re-read per focused session so the control never shows a stale flag
  // after a switch; the flag itself lives backend-side (durable sidecar).
  // Presence of the prop (not truthiness) gates the toggle: `null` means
  // "no focused id — the backend resolves the ACTIVE session".
  const [memoryBypassed, setMemoryBypassed] = useState(false)
  useEffect(() => {
    let cancelled = false
    if (!sessionScoped) {
      setMemoryBypassed(false)
      return
    }
    api.getSessionMemoryBypass(sessionId ?? null)
      .then(disabled => { if (!cancelled) setMemoryBypassed(Boolean(disabled)) })
      .catch(() => { if (!cancelled) setMemoryBypassed(false) })
    return () => { cancelled = true }
  }, [sessionScoped, sessionId])
  const handleMemoryBypassToggle = async () => {
    const next = !memoryBypassed
    setMemoryBypassed(next)
    try {
      await api.setSessionMemoryBypass(sessionId ?? null, next)
    } catch (err) {
      // Optimistic flip rolled back — the backend state stays authoritative.
      setMemoryBypassed(!next)
      toastError(t('chat.input.memoryBypass.failed'), err)
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
    // P0-3 preflight — flag refused paths on the chips BEFORE the send.
    // Strictly advisory: a failed call OR a malformed payload (the mocked
    // invoke in tests resolves `undefined`) must degrade to "no marking",
    // never break the composer.
    api.checkAttachmentPaths(paths)
      .then(checks => {
        if (!Array.isArray(checks)) return
        setPathIssues(prev => {
          const next = { ...prev }
          for (const c of checks) {
            if (!c || typeof c.path !== 'string') continue
            if (c.ok || !c.reason) delete next[c.path]
            else next[c.path] = c.reason
          }
          return next
        })
        // G3b P1-4 — chip badges from the same advisory response.
        setPathReports(prev => {
          const next = { ...prev }
          for (const c of checks) {
            if (!c || typeof c.path !== 'string') continue
            if (c.extraction) next[c.path] = c.extraction
            else delete next[c.path]
          }
          return next
        })
        // R7-③ threshold hybrid — large parseable documents come back with
        // `deferred_parse` and NO extraction: the chip shows the honest
        // "parsed on send" placeholder until the send receipt lights the
        // real badge.
        setDeferredParsePaths(prev => {
          const next = { ...prev }
          for (const c of checks) {
            if (!c || typeof c.path !== 'string') continue
            if (c.deferred_parse) next[c.path] = true
            else delete next[c.path]
          }
          return next
        })
      })
      .catch(() => {})
    // B9' Files page: index the attachment references so they surface in the
    // reference-style library. Fire-and-forget — a failed index write must
    // never interrupt the attach flow (offline, scope errors, demo mode).
    for (const p of paths) {
      api.registerFileIndexEntry(p, 'attachment').catch(() => {})
    }
  }
  // B0 P0-2: the drag-drop subscription outlives single renders, so it
  // dispatches through a latest-ref instead of re-subscribing on every
  // attachments change.
  const mergePathsRef = useRef(mergePaths)
  useEffect(() => {
    mergePathsRef.current = mergePaths
  })

  // G3b P1-6 — clipboard-image paste. Screenshot tools put `image/*` File
  // items on `clipboardData`; those are persisted via `save_pasted_image`
  // and funneled into the normal attachment list (preflight included). A
  // paste with NO image items falls through untouched — the default text
  // insertion (and IME composition) behavior is never intercepted.
  const handlePastedImages = async (files: File[]) => {
    for (const file of files) {
      const ext = PASTE_IMAGE_MIME_TO_EXT[file.type]
      if (!ext) {
        toastError(
          t('chat.input.paste.failed'),
          intl.formatMessage({ id: 'chat.input.paste.unsupported' }, { type: file.type || 'unknown' }),
        )
        continue
      }
      if (file.size > MAX_PASTED_IMAGE_BYTES) {
        // Over the shared image cap: fail fast client-side with the i18n
        // message (the backend re-checks via the same 10 MiB limit).
        toastError(
          t('chat.input.paste.failed'),
          intl.formatMessage({ id: 'chat.input.paste.tooLarge' }),
        )
        continue
      }
      try {
        const dataBase64 = await blobToBase64(file)
        const savedPath = await api.savePastedImage(dataBase64, ext)
        mergePathsRef.current([savedPath])
      } catch (err) {
        toastError(t('chat.input.paste.failed'), err)
      }
    }
  }
  const handlePaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const images = imageFilesFromClipboard(e.clipboardData)
    if (images.length === 0) return
    e.preventDefault()
    void handlePastedImages(images)
  }

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

  // B1 §4-12: the chat search bar's Esc/close hands focus back to whatever
  // surface the user left — which is usually this textarea.
  useEffect(() => {
    const onFocusComposer = () => textareaRef.current?.focus()
    window.addEventListener('shannon:focus-composer', onFocusComposer)
    return () => window.removeEventListener('shannon:focus-composer', onFocusComposer)
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

  // A-22 — terminal-style input-history recall (ArrowUp/ArrowDown). The ring
  // itself is global (`shannon.inputHistory`, maintained by Chat.tsx on every
  // accepted send/queue join); this component only walks it. Entering is
  // allowed only when BOTH menus are closed (guaranteed structurally: the
  // mention/slash blocks above capture the arrows while open) AND the input
  // is empty or the caret sits at the very start of the text (position 0 =
  // first line, first column) — exactly the states where ArrowUp has no
  // caret-moving job, so multi-line navigation is never hijacked and screen
  // readers keep their line-by-line textbox behavior.
  const [historyBrowse, setHistoryBrowse] = useState<{
    index: number // position in historyRef.current — length-1 is the newest
    snapshot: string // composer text (+ caret) from before browsing started;
    snapshotCaret: number // Down past the newest entry restores this现场.
  } | null>(null)
  // Re-read once per ENTRY into history mode (not cached per mount) so an
  // entry recorded by a send during this mount is always recallable.
  const historyRef = useRef<string[]>([])
  // The value the last recall wrote. A `value` change NOT authored by the
  // browse cursor (send clear, draft push, skill fill, session switch) ends
  // the browse session — the on-screen text is no longer what the cursor
  // points at, and Down-restore would clobber it.
  const historyShownRef = useRef<string | null>(null)
  // Push an entry (or the entry snapshot) into the composer, caret at its
  // end — or at `caret` when restoring the pre-browse snapshot.
  const showHistoryEntry = (text: string, caret?: number) => {
    historyShownRef.current = text
    onChange(text)
    const pos = caret ?? text.length
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (!el) return
      el.focus()
      el.setSelectionRange(pos, pos)
    })
  }
  useEffect(() => {
    setHistoryBrowse(prev => (prev && value !== historyShownRef.current ? null : prev))
  }, [value])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // IME guard first: swallow only the send/execute keys so the candidate
    // window keeps them; navigation keys pass through untouched.
    if (isCompositionKey(e)) {
      if (e.key === 'Enter' || e.key === 'Tab') e.preventDefault()
      return
    }
    // GB P2-10b: the @-mention menu captures the navigation keys while open
    // (checked before the slash menu — the two never match simultaneously).
    if (mentionMatches.length > 0) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        const delta = e.key === 'ArrowDown' ? 1 : -1
        setMentionActive(i => (i + delta + mentionMatches.length) % mentionMatches.length)
        return
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault()
        const picked = mentionMatches[mentionActive] ?? mentionMatches[0]
        if (picked) commitMention(picked)
        return
      }
      if (e.key === 'Escape') {
        e.preventDefault()
        setMentionDismissed(true)
        return
      }
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
        const picked = slashMatches[slashActive] ?? slashMatches[0]
        if (picked) {
          if (picked.kind === 'command') executeSlash(picked.command)
          else executeSkill(picked.skill)
        }
        return
      }
      if (e.key === 'Escape') {
        e.preventDefault()
        setSlashDismissed(true)
        return
      }
    }
    // A-22 — input-history recall. Reachable only when both menus are closed
    // (each captures the arrows above). While browsing, Up/Down walk the
    // ring; Down past the newest entry restores the pre-browse text + caret
    // and exits. Every branch prevents the default so the caret stays where
    // the recall put it. The IME guard at the top already swallowed arrows
    // belonging to an in-flight composition.
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      const history = historyRef.current
      if (historyBrowse && history.length > 0) {
        e.preventDefault()
        if (e.key === 'ArrowUp') {
          // At the oldest entry Up is a no-op (terminal-ring semantics).
          if (historyBrowse.index > 0) {
            const idx = historyBrowse.index - 1
            setHistoryBrowse({ ...historyBrowse, index: idx })
            showHistoryEntry(history[idx])
          }
        } else if (historyBrowse.index >= history.length - 1) {
          setHistoryBrowse(null)
          showHistoryEntry(historyBrowse.snapshot, historyBrowse.snapshotCaret)
        } else {
          const idx = historyBrowse.index + 1
          setHistoryBrowse({ ...historyBrowse, index: idx })
          showHistoryEntry(history[idx])
        }
        return
      }
      if (e.key === 'ArrowUp') {
        const el = textareaRef.current
        const caret = el?.selectionStart ?? value.length
        if (value.length === 0 || caret === 0) {
          const ring = loadInputHistory()
          if (ring.length > 0) {
            e.preventDefault()
            historyRef.current = ring
            const idx = ring.length - 1
            setHistoryBrowse({ index: idx, snapshot: value, snapshotCaret: caret })
            showHistoryEntry(ring[idx])
          }
        }
      }
      return
    }
    // Enter -> send (while streaming: queue, GB P2-10a); Shift+Enter ->
    // newline; Ctrl/Cmd+Enter -> send — or, while streaming, the IMMEDIATE
    // steer tier (interrupt now). Matches VS Code's Ctrl+Enter convention;
    // preserves the legacy Enter-to-send UX.
    if (e.key === 'Enter' && !e.shiftKey && !(e.ctrlKey || e.metaKey)) {
      e.preventDefault()
      onSend()
    } else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.shiftKey) {
      e.preventDefault()
      if (isQuerying && onSteer) onSteer()
      else onSend()
    }
    // Escape priority: exit message edit > cancel the running query.
    if (e.key === 'Escape') {
      if (onCancelEdit) {
        e.preventDefault()
        onCancelEdit()
        return
      }
      if (isQuerying) {
        e.preventDefault()
        onCancelQuery()
      }
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
  // GB P2-4: the pill reads the SHARED five-tier table (same source as
  // Settings → General). Values outside the table (engine-only aliases)
  // render honestly via the fallback instead of masquerading as Suggest.
  const selectedMode = approvalModeOption(currentMode)

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
      // Same guard shape as hooks/useKeyboardShortcuts.ts. No isQuerying
      // gate: while a run streams is exactly when focusing the composer to
      // queue a prompt is most useful (B1 §4-9).
      if (e.key === '/') {
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

  /* "+" menu — attachments and the two inline tools, one click each. */
  const [plusOpen, setPlusOpen] = useState(false)
  // B2 v1: "Build a presentation" — outline confirmation dialog. Generate
  // pushes a draft into THIS composer (below) and never sends by itself.
  const [pptOpen, setPptOpen] = useState(false)

  // B2 v1: composer drafts from surface components (PPT outline today).
  // Insertion appends after any existing draft text; the ref keeps the
  // listener from re-subscribing on every keystroke.
  const valueRef = useRef(value)
  useEffect(() => {
    valueRef.current = value
  })
  useComposerDraftListener(text => {
    const current = valueRef.current
    const next = current.trim() ? `${current.replace(/\s+$/, '')}\n\n${text}` : text
    // Sync the ref immediately: the pending-draft queue (G5 P0-6) can flush
    // several drafts in one tick, before React re-renders with the first
    // onChange — without this, draft #2 would read the stale value and
    // overwrite draft #1 instead of appending after it.
    valueRef.current = next
    onChange(next)
    textareaRef.current?.focus()
  })

  // SessionUsageDialog — composer 模型 chip 旁的会话用量入口(2026-09
  // 三项 UX 修复 #3)。弹框点击后才挂载,首屏零开销;打开期间跟随父
  // 组件透传的 streaming usageTick 实时刷新 breakdown。
  const [usageOpen, setUsageOpen] = useState(false)
  const plusItems: DropdownMenuItem[] = [
    { id: 'attach', label: t('chat.input.attach.aria'), icon: 'attach_file', onSelect: () => { setPlusOpen(false); void handleAttachClick() } },
    { id: 'ppt', label: t('office.ppt.title'), icon: 'slideshow', onSelect: () => { setPlusOpen(false); setPptOpen(true) } },
    { id: 'quickfix', label: t('nav.quickFix'), icon: 'build', onSelect: () => { setPlusOpen(false); onOpenQuickFix() } },
    { id: 'editor', label: t('nav.editor'), icon: 'code', onSelect: () => { setPlusOpen(false); onOpenEditor() } },
  ]

  /* Char count UI */
  const charCount = value.length
  const showCharCount = charCount >= CHAR_SHOW_AT
  const isOverSoftWarn = charCount >= CHAR_SOFT_WARN_AT

  // GB P2-10a: same gate as the idle send button — the queue/steer buttons
  // only appear when there is something to deliver.
  const hasSteerableContent = value.trim().length > 0 || attachedFiles.length > 0

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
          className="absolute left-0 right-0 bottom-full mb-sm z-modal rounded-2xl border border-outline-variant/30 bg-surface-container-low shadow-e3 overflow-hidden"
        >
          <ul className="max-h-64 overflow-y-auto py-xs">
            {slashMatches.map((item, i) => {
              const key = item.kind === 'command' ? `cmd-${item.command.name}` : `skill-${item.skill.name}`
              const icon = item.kind === 'command' ? item.command.icon : 'auto_fix'
              const trigger = item.kind === 'command' ? `/${item.command.name}` : item.skill.trigger
              const description =
                item.kind === 'command'
                  ? t(item.command.descriptionKey)
                  : item.skill.description
              return (
                <li key={key}>
                  <button
                    type="button"
                    role="option"
                    id={slashOptionId(key)}
                    aria-selected={i === slashActive}
                    onMouseDown={e => {
                      e.preventDefault()
                      if (item.kind === 'command') executeSlash(item.command)
                      else executeSkill(item.skill)
                    }}
                    onMouseEnter={() => setSlashActive(i)}
                    className={cn(
                      'w-full flex items-center gap-sm px-md py-xs text-left cursor-pointer transition-colors',
                      i === slashActive ? 'bg-surface-container-high' : 'hover:bg-surface-container',
                    )}
                  >
                    <span className="material-symbols-outlined icon-sm text-primary shrink-0">{icon}</span>
                    <span className="font-mono text-label-md text-on-surface shrink-0">{trigger}</span>
                    {item.kind === 'skill' && (
                      <span className="text-label-xs px-xs py-[1px] rounded-full bg-tertiary-container/50 text-on-tertiary-container font-bold shrink-0">
                        {t('slash.menu.skillTag')}
                      </span>
                    )}
                    <span className="font-label-sm text-on-surface-variant truncate flex-1">{description}</span>
                  </button>
                </li>
              )
            })}
          </ul>
          <div className="px-md py-xs border-t border-outline-variant/20 text-label-xs text-on-surface-variant">
            {t('slash.menu.hint')}
          </div>
        </div>
      )}

      {/* P2-9, revised after integration review: the composer keeps its
          implicit multi-line `textbox` role — a permanent `role="combobox"`
          mislabels the 99%-of-the-time plain text area for assistive tech
          (and broke the `getByRole('textbox', { name: 'Message' })` E2E
          contract). Menu state is announced through this polite status
          region instead: open/count/selection updates are all render-driven. */}
      {slashOpen && (
        <span role="status" className="sr-only">
          {intl.formatMessage(
            { id: 'chat.input.slashMenu.status' },
            {
              count: slashMatches.length,
              current:
                slashMatches[slashActive] !== undefined
                  ? slashMatches[slashActive].kind === 'command'
                    ? `/${slashMatches[slashActive].command.name}`
                    : slashMatches[slashActive].skill.trigger
                  : '',
            },
          )}
        </span>
      )}

      {/* GB P2-10b — @ file-reference popover. Same shape and protocol as
          the slash menu: keyboard-first listbox, Escape dismisses until the
          query changes, selection inserts plain text. */}
      {mentionMatches.length > 0 && (
        <div
          id={`${mentionListboxId}`}
          role="listbox"
          aria-label={t('chat.input.mention.menu.aria')}
          className="absolute left-0 right-0 bottom-full mb-sm z-modal rounded-2xl border border-outline-variant/30 bg-surface-container-low shadow-e3 overflow-hidden"
        >
          <ul className="max-h-64 overflow-y-auto py-xs">
            {mentionMatches.map((path, i) => (
              <li key={path}>
                <button
                  type="button"
                  role="option"
                  id={`${mentionListboxId}-opt-${i}`}
                  aria-selected={i === mentionActive}
                  onMouseDown={e => {
                    e.preventDefault()
                    commitMention(path)
                  }}
                  onMouseEnter={() => setMentionActive(i)}
                  className={cn(
                    'w-full flex items-center gap-sm px-md py-xs text-left cursor-pointer transition-colors',
                    i === mentionActive ? 'bg-surface-container-high' : 'hover:bg-surface-container',
                  )}
                >
                  <span className="material-symbols-outlined icon-sm text-primary shrink-0">description</span>
                  <span className="font-mono text-label-md text-on-surface truncate flex-1" title={path}>
                    {relativeToWorkingDir(path, sessionWorkingDir)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
          <div className="px-md py-xs border-t border-outline-variant/20 text-label-xs text-on-surface-variant">
            {t('chat.input.mention.hint')}
          </div>
        </div>
      )}
      {mentionMatches.length > 0 && (
        <span role="status" className="sr-only">
          {intl.formatMessage(
            { id: 'chat.input.mention.status' },
            { count: mentionMatches.length, current: mentionMatches[mentionActive] ?? '' },
          )}
        </span>
      )}

      {isDragging && (
        // Scrim-style drag veil (遮罩) over the composer while a file drag is
        // in flight — intentional direct backdrop-blur, G1 exempt.
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
          data-testid="plan-mode-banner"
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
            className="rounded-sm hover:bg-tertiary/20 shrink-0"
          >
            <span className="material-symbols-outlined icon-sm">close</span>
          </Button>
        </div>
      )}

      {memoryBypassed && (
        // P2-5 — the explicit "this session does not use memory" notice.
        // Rendered above the input (plan-banner shape) so the bypass is
        // spelled out, not just color-coded on the toggle.
        <div
          role="status"
          data-testid="memory-bypass-banner"
          className="flex items-center gap-xs px-md py-xs bg-primary-container/50 border-b border-primary/20 rounded-t-2xl text-on-primary-container"
        >
          <span className="material-symbols-outlined icon-sm shrink-0">psychology</span>
          <span className="font-label-sm truncate flex-1">{t('chat.input.memoryBypass.banner')}</span>
        </div>
      )}

      {voice.state !== 'idle' && (
        <div className="flex items-center justify-center py-sm bg-primary/5 rounded-t-2xl">
          <VoiceOrb state={voice.state} />
        </div>
      )}

      {/* P0-3 — the preflight found NO working directory: the attachment
          domain is undefined and every chip would be refused on send. Point
          at Settings (deep link) instead of letting the user hit a wall. */}
      {hasNoWorkingDirIssue && (
        <div
          role="status"
          data-testid="no-working-dir-banner"
          className="flex items-start gap-xs px-md py-xs bg-warning-container/60 border-b border-warning/30 rounded-t-2xl text-on-warning-container"
        >
          <span className="material-symbols-outlined icon-sm shrink-0 mt-[2px]">folder_off</span>
          <div className="flex-1 min-w-0">
            <div className="font-label-sm">{t('chat.attach.noWorkingDir.banner')}</div>
            <div className="font-label-xs text-on-warning-container/80 mt-[1px]">
              {t('chat.attach.reason.noWorkingDir')}
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            data-testid="no-working-dir-open-settings"
            onClick={() => navigate('/settings')}
            className="rounded-sm hover:bg-warning/20 shrink-0"
          >
            <span className="material-symbols-outlined icon-sm">settings</span>
            {t('chat.attach.noWorkingDir.openSettings')}
          </Button>
        </div>
      )}

      {/* A1a — sits above the input box so the honest "content was NOT sent"
          line is read before the user hits send. Same banner shape as the
          plan-mode strip, warning palette. */}
      {showUnparsedNotice && (
        <div
          role="status"
          className="flex items-start gap-xs px-md py-xs bg-warning-container/60 border-b border-warning/30 rounded-t-2xl text-on-warning-container"
        >
          <span className="material-symbols-outlined icon-sm shrink-0 mt-[2px]">info</span>
          <div className="flex-1 min-w-0">
            <div className="font-label-sm">{t('chat.input.attach.unsupportedType')}</div>
            <div className="font-label-xs text-on-warning-container/80 mt-[1px]">
              {t('chat.input.attach.unsupportedHint')}
            </div>
          </div>
          <Button
            variant="ghost"
            size="icon-xs"
            onClick={() => setUnparsedDismissed(true)}
            aria-label={t('chat.message.attachment.close')}
            title={t('chat.message.attachment.close')}
            className="rounded-sm hover:bg-warning/20 shrink-0"
          >
            <span className="material-symbols-outlined icon-sm">close</span>
          </Button>
        </div>
      )}

      <div className="flex flex-col">
        {attachedFiles.length > 0 && (
          <div className="flex flex-wrap items-center gap-xs px-md pt-md">
            {attachedFiles.map((path, i) => (
              <AttachmentChip
                key={path}
                path={path}
                issue={pathIssues[path]}
                extraction={pathReports[path]}
                deferredParse={Boolean(deferredParsePaths[path])}
                onRemove={() => onAttach(attachedFiles.filter((_, idx) => idx !== i))}
              />
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
                ? t('chat.input.queued.placeholder')
                : sessionWorkingDir
                  ? intl.formatMessage({ id: 'chat.input.placeholder.project' }, { dir: basename(sessionWorkingDir) })
                  : t('chat.input.placeholder.empty')
            }
            aria-label={t('chat.input.ariaLabel')}
            value={value}
            onChange={e => {
              trackCaret(e.currentTarget)
              // A-22: a DOM-level change is user input (typed, pasted, IME)
              // — history browsing ends and the snapshot is abandoned.
              // Programmatic value writes (the recall itself) never fire
              // this handler, so navigation is unaffected.
              setHistoryBrowse(null)
              onChange(e.target.value)
            }}
            onKeyUp={e => trackCaret(e.currentTarget)}
            onClick={e => trackCaret(e.currentTarget)}
            onSelect={e => trackCaret(e.currentTarget)}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
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
                data-testid="composer-plus-menu"
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

            <Select value={selectedMode.value} onValueChange={handleModeChange}>
              <SelectTrigger
                size="sm"
                data-testid="approval-mode-pill"
                aria-label={t('chat.input.mode.label')}
                title={`${selectedMode.rawLabel ? selectedMode.rawLabel : t(selectedMode.descriptionKey)} · ${t('chat.input.mode.highRiskNote')}`}
                className={cn('rounded-full border', selectedMode.tone, 'bg-transparent hover:bg-surface-container-low/50 transition-colors')}
              >
                <span className="material-symbols-outlined icon-sm">{selectedMode.icon}</span>
                {/* Render the matched mode's local label, not the raw value
                    — an unknown approval_mode (e.g. a CLI-only alias) shows
                    verbatim via rawLabel instead of bleeding into the pill
                    chrome as a translated label it doesn't have. */}
                <SelectValue placeholder={t('chat.input.mode.label')}>
                  {() => <span className="truncate">{selectedMode.rawLabel ?? t(selectedMode.labelKey)}</span>}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                {APPROVAL_MODES.map(mode => (
                  <SelectItem key={mode.value} value={mode.value}>
                    <div className="flex items-start gap-xs py-0.5">
                      <span className="material-symbols-outlined icon-sm mt-0.5" aria-hidden="true">{mode.icon}</span>
                      <span className="min-w-0">
                        <span className="block font-label-md text-on-surface whitespace-nowrap">{t(mode.labelKey)}</span>
                        <span className="block font-label-xs text-on-surface-variant whitespace-normal">{t(mode.descriptionKey)}</span>
                      </span>
                    </div>
                  </SelectItem>
                ))}
                {/* GB P2-4: the danger note travels with the switcher — the
                    tier only moves the auto-approve baseline; High-risk
                    actions keep their confirmation prompt regardless. */}
                <div
                  role="note"
                  className="mx-sm my-xs border-t border-outline-variant/20 pt-xs font-label-xs text-on-surface-variant whitespace-normal"
                >
                  {t('chat.input.mode.highRiskNote')}
                </div>
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

            {/* P2-5 — 临时会话 toggle (memory bypass). aria-pressed + a
                distinct active treatment so the state is never silent; the
                status strip below spells it out. */}
            <Button
              variant="ghost"
              aria-pressed={memoryBypassed}
              data-testid="memory-bypass-toggle"
              aria-label={memoryBypassed
                ? t('chat.input.memoryBypass.on.aria')
                : t('chat.input.memoryBypass.off.aria')}
              title={memoryBypassed
                ? t('chat.input.memoryBypass.on.title')
                : t('chat.input.memoryBypass.off.title')}
              className={cn(
                'p-md shrink-0 transition-colors',
                memoryBypassed
                  ? 'bg-primary-container/70 text-on-primary-container hover:bg-primary-container'
                  : 'text-on-surface-variant hover:text-primary',
              )}
              onClick={() => void handleMemoryBypassToggle()}
            >
              <span className="material-symbols-outlined icon-md" aria-hidden="true">psychology</span>
            </Button>

            <Select
              value={currentModel?.id ?? ''}
              onValueChange={value => {
                // Namespaced menu values never become the Select value:
                // reasoning effort (`effort:`) and the R2-1 session actions
                // (`set-default` / `clear-override`) commit and return.
                if (value && value.startsWith('effort:')) {
                  void handleEffortChange(value.slice('effort:'.length))
                  return
                }
                if (value === 'set-default') {
                  void handleSetAsDefault()
                  return
                }
                if (value === 'clear-override') {
                  void handleClearSessionOverride()
                  return
                }
                if (value) void handleModelSwitch(value)
              }}
            >
              <SelectTrigger
                size="sm"
                data-testid="model-chip-trigger"
                aria-label={t('chat.input.model.label')}
                title={sessionOverride
                  ? intl.formatMessage(
                      { id: 'chat.input.model.sessionTitle' },
                      { model: currentModel?.name ?? sessionOverride.model },
                    )
                  : t('chat.input.model.title')}
                className="max-w-[170px] rounded-full border border-outline-variant/50 bg-transparent hover:bg-surface-container-low/50 transition-colors"
              >
                <span className="material-symbols-outlined icon-sm">smart_toy</span>
                {/* Render the effective model NAME — the session override's
                    model when one is active (with a "· session" suffix so the
                    override is never silent), else the global default. */}
                <SelectValue placeholder={status?.model || t('chat.input.model.label')}>
                  {(value: unknown) => {
                    // Reflect a non-default reasoning effort on the chip —
                    // the Select's value is always a model id (effort picks
                    // commit via `effort:` in onValueChange but never become
                    // the Select value), so read it from config directly.
                    const eff = effortOptions.find(e => e.value === currentEffort)
                    const m = modelList.find(x => x.id === value)
                    const name = m?.name
                      ?? sessionOverride?.model
                      ?? status?.model
                      ?? t('chat.input.model.label')
                    let label = name
                    if (eff && eff.value !== 'medium') {
                      label = `${label} · ${eff.label}`
                    }
                    if (sessionOverride) {
                      label = `${label} · ${t('chat.input.model.sessionSuffix')}`
                    }
                    return label
                  }}
                </SelectValue>
              </SelectTrigger>
              {/* R2-3: widened past the chip's anchor width so the context/
                  price meta fits on the model rows. */}
              <SelectContent className="w-[320px]">
                {modelList.map(m => (
                  <SelectItem key={m.id} value={m.id} data-testid={`model-option-${m.id}`}>
                    <span className="flex w-full min-w-0 items-center gap-xs">
                      <span className="font-mono truncate">{m.name}</span>
                      {/* R2-3: vision dot — rendered only from real catalog
                          metadata; unknown renders nothing (never guessed). */}
                      {m.vision === true && (
                        <span
                          aria-label={t('chat.input.model.vision')}
                          title={t('chat.input.model.vision')}
                          className="inline-block size-1.5 shrink-0 rounded-full bg-primary"
                        />
                      )}
                      <span className="ml-auto shrink-0 whitespace-nowrap font-label-xs text-on-surface-variant tabular-nums">
                        {modelPickerMeta(m)}
                      </span>
                    </span>
                  </SelectItem>
                ))}
                {modelList.length > 0 && (
                  <div role="presentation" className="mx-sm my-xs border-t border-outline-variant/20" />
                )}
                {/* R2-1: session model actions — "Set as default" promotes
                    the chip's current model to the engine-global default
                    (the pre-R2-1 chip behavior); "Reset to default" (shown
                    only while an override is active) re-inherits it. */}
                {sessionScoped && (
                  <>
                    <div role="presentation" className="px-sm pt-0 pb-xs font-label-xs uppercase tracking-wider text-on-surface-variant">
                      {t('chat.input.model.sessionSection')}
                    </div>
                    <SelectItem value="set-default" data-testid="model-action-set-default">
                      <span className="flex items-center gap-xs">
                        <span className="material-symbols-outlined icon-sm" aria-hidden="true">push_pin</span>
                        {t('chat.input.model.setDefault')}
                      </span>
                    </SelectItem>
                    {sessionOverride && (
                      <SelectItem value="clear-override" data-testid="model-action-clear-override">
                        <span className="flex items-center gap-xs">
                          <span className="material-symbols-outlined icon-sm" aria-hidden="true">restart_alt</span>
                          {t('chat.input.model.resetSession')}
                        </span>
                      </SelectItem>
                    )}
                  </>
                )}
                <div role="presentation" className="px-sm pt-0 pb-xs font-label-xs uppercase tracking-wider text-on-surface-variant">
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
            {/* B4 P2-5: no STT provider (no MediaRecorder/getUserMedia) → no
                mic button; a control that only opens a doomed recording is
                worse than none. */}
            {voice.supported && (
              <MicButton
                state={voice.state}
                disabled={isQuerying}
                onStart={() => void voice.startRecording()}
                onStop={() => void voice.stopRecording()}
              />
            )}

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
              <>
                {/* GB P2-10a: while streaming the send slot becomes the
                    QUEUE button (Enter does the same) — the label says the
                    message goes out after the current turn ends. Only shown
                    when there is something to queue. */}
                {hasSteerableContent && (
                  <Button
                    aria-label={t('chat.input.queue.aria')}
                    title={t('chat.input.queue.title')}
                    className="bg-primary text-on-primary p-3 rounded-xl active:scale-95 hover:shadow-e2 transition-all"
                    onClick={onSend}
                  >
                    <span className="material-symbols-outlined icon-md">low_priority</span>
                  </Button>
                )}
                {hasSteerableContent && onSteer && (
                  // The second tier: interrupt the running turn and deliver
                  // now (also Ctrl/Cmd+Enter). Secondary styling — queueing
                  // stays the default path.
                  <Button
                    variant="outline"
                    aria-label={t('chat.input.steer.aria')}
                    title={t('chat.input.steer.title')}
                    className="p-3 rounded-xl active:scale-95 transition-all text-primary border-primary/40 hover:bg-primary/10"
                    onClick={onSteer}
                  >
                    <span className="material-symbols-outlined icon-md">bolt</span>
                  </Button>
                )}
                {/* S-3/A-18 companion (R4 group 7): while the cancel IPC is
                    tearing the run down, the second stop is a backend no-op
                    (the token was already taken) — render a disabled
                    spinner state so the press visibly registered. */}
                <Button
                  aria-label={t(cancelInFlight ? 'chat.input.stop.cancelling.aria' : 'chat.input.stop.aria')}
                  title={t(cancelInFlight ? 'chat.input.stop.cancelling.title' : 'chat.input.stop.aria')}
                  className="bg-error/80 text-on-error p-3 rounded-xl active:scale-95 transition-all disabled:opacity-60 disabled:cursor-wait"
                  onClick={onCancelQuery}
                  disabled={cancelInFlight}
                >
                  <span className={cn('material-symbols-outlined icon-md', cancelInFlight && 'animate-spin')}>
                    {cancelInFlight ? 'progress_activity' : 'stop'}
                  </span>
                </Button>
              </>
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
      {/* B2 v1 — PPT outline confirmation. Generate pushes a composer draft
          (review-then-send); close/cancel pushes nothing. */}
      {pptOpen && <PptOutlineDialog open onClose={() => setPptOpen(false)} />}
    </div>
  )
}
