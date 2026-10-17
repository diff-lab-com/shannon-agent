import { useState, useRef, useEffect, useLayoutEffect, useId, useCallback, useMemo, Fragment } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { open } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { Menu } from '@base-ui/react/menu'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { DropdownMenu, DropdownMenuSeparator, type DropdownMenuEntry } from '@/components/ui/dropdown-menu'
import { useCatalog } from '@/context/CatalogContext'
import { useVoice } from '@/hooks/useVoice'
import { MicButton } from '@/components/voice/MicButton'
import { VoiceOrb } from '@/components/voice/VoiceOrb'
import AttachmentChip from '@/components/chat/AttachmentChip'
import SessionUsageDialog from '@/components/chat/SessionUsageDialog'
import PptOutlineDialog from '@/components/chat/PptOutlineDialog'
import { useComposerDraftListener } from '@/lib/composerBridge'
import { loadInputHistory } from '@/lib/inputHistory'
import { findSlashCommand, isSlashQuery, slashArgsToken, type SlashCommand } from '@/lib/slash/commands'
import { fetchSlashSkills, mergeSlashMenu, type SlashMenuItem, type SlashSkillEntry } from '@/lib/slash/skills'
import {
  activeMentionQuery,
  caretAfterMentionInsert,
  filterMentionCandidates,
  flattenFileTree,
  insertMention,
  relativeToWorkingDir,
} from '@/lib/fileMention'
import { imageFilesFromClipboard, clipboardHasText, blobToBase64, PASTE_IMAGE_MIME_TO_EXT, MAX_PASTED_IMAGE_BYTES } from '@/lib/pasteImage'
import * as api from '@/lib/tauri-api'
import type { RejectedAttachmentReason, AttachmentExtractionReport, SendCostEstimate, UsagePayload } from '@/types'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'
import { ModelPickerRowContent } from '@/components/shared/ModelPickerRow'
import { modelWhyFor, type ModelWhyContext } from '@/lib/modelWhy'
import { writeGlobalModelDefault } from '@/lib/modelSwitch'
import { APPROVAL_MODES, PLAN_OPTION, APPROVAL_MODE_CYCLE, approvalModeOption, normalizeApprovalMode } from '@/lib/approvalModes'
import { VISION_IMAGE_EXTENSIONS as IMAGE_EXTENSIONS } from '@/lib/fileRefs'

/**
 * R2-P1-2 attachment honesty — exactly the image formats the backend's
 * multimodal whitelist turns into image blocks (png/jpeg/gif/webp; see
 * `is_vision_image_mime` in commands.rs). bmp/svg used to sit here too, so
 * the picker advertised formats the model never sees: the file attached,
 * the backend dropped it from the image blocks with no rejected receipt.
 * The drag-drop and paste paths bypass this filter by design — the backend
 * gate + preflight badge (`unsupported_type`) catch those.
 *
 * The set lives in `@/lib/fileRefs` as the single source of truth (the
 * S2-4a vision pre-check's image filter reads the same table); re-exported
 * here for the picker's file-dialog filter.
 */
export { IMAGE_EXTENSIONS }

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

/**
 * Aurora redesign 2026-10 (02 页 P1) — the composer's execution-mode control.
 *
 * 02c 设计提案裁决 D3(2026-10-14,二次修正): the control is a SINGLE chip
 * showing the ACTIVE mode (icon + label), opening the approval ladder as a
 * dropdown (逐档说明 + high-risk note). The former four-stop segmented
 * control and its 1200px breakpoint fold (SEGMENTED_MODE_MIN_WIDTH_PX /
 * useMinWindowWidth) are gone — one form at every width, which is exactly
 * the Select shape the <1200px fallback already rendered; the testid stays
 * `approval-mode-pill`. Engine support unchanged: `plan` is a legal
 * `approval_mode` value (crates/shannon-engine permissions.rs
 * ApprovalMode::Plan; desktop/src/commands.rs parse_approval_mode maps
 * "plan" → Plan). The expert modes (readonly/dontAsk/bypassPermissions)
 * stay Settings-only (lib/approvalModes).
 */

/* G1-A parity batch (2026-10-15 review §1.2 chat): the contextual
 * "Working in {dir}" placeholder (and its basename helper) is retired — the
 * design's composer carries a plain-text placeholder only, and the working
 * directory already has its own surface in the ChatStatusBar's directory
 * segment, so nothing semantic is lost. The session name/dir is NOT re-homed
 * elsewhere for the composer; it stays in the status bar alone. */

/* Char-count thresholds.
 *   showAt — start showing the live counter
 *   softWarnAt — visually promote (orange/yellow) without blocking
 * Beyond softWarn the counter is just a louder warning; the user can
 * still hit send. Hard limits should go through the Rust backend. */
const CHAR_SHOW_AT = 2000
const CHAR_SOFT_WARN_AT = 8000

/* H2 P1-4 — pasted text at or above this length is converted into a .txt
 * attachment instead of being inserted into the composer. Rationale: 8 KiB
 * is where pasted content stops being a quotable snippet and becomes
 * reference material — it sits right at the composer's own soft-warn line
 * (CHAR_SOFT_WARN_AT = 8000, the "this will not read well as a message"
 * signal) and amounts to roughly 2–6k tokens for a mixed CJK/Latin paste
 * (≈1 token per 1–3 chars), i.e. the order of magnitude where one paste
 * visibly eats the context window. Cherry Studio / Chatbox ship the same
 * auto-attach behavior for long pastes. Below the threshold the paste is
 * left alone — the default insertion path is untouched. Exported for the
 * paste-conversion test. */
export const MAX_INLINE_PASTE_CHARS = 8192

/* S3-6 — pre-send cost estimate debounce. The estimate command is cheap
 * (cached context + attachment counts backend-side), but per-keystroke IPC
 * is still waste: re-estimate 300 ms after the draft settles. Exported for
 * the debounce test. */
export const ESTIMATE_DEBOUNCE_MS = 300

/* S3-6 — the budget linkage threshold: when the projected send (spent so
 * far + the estimate's high end) reaches this share of the session budget,
 * the estimate row turns warning-toned and names the share. Copy-only
 * linkage — BudgetBanner stays the single banner (no second one here). */
const ESTIMATE_BUDGET_WARN_RATIO = 0.8

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
  /** 02c 裁决 D4 — the SAME streaming Usage payload, typed, feeding the
   *  composer's context-% chip (icon + percent beside the model chip). The
   *  percent computes exactly like ChatStatusBar's old segment:
   *  (input+output)/context_total, and hides whenever the engine-resolved
   *  window or the token counts are absent — an unknown window never
   *  renders a guessed percentage (honesty contract). */
  usage?: UsagePayload | null
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
  usage,
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

  // P2-3 — menu ownership. The composer keeps its implicit multi-line
  // `textbox` role (see the P2-9 revision note below — no combobox, no
  // aria-activedescendant); while a menu is open the textarea points at
  // the visible listbox via aria-controls, and open/count/selection
  // changes are announced through the polite status regions instead.
  // The two menus are mutually exclusive by construction, so one
  // conditional covers both.
  const slashListboxId = useId()
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

  // H2 P1-2 — the "/cmd args" parameter state. The bare-token regex stopped
  // matching (whitespace plus more text after it), so the menu is gone; when
  // the FIRST token exactly hits the static table or an installed skill, a
  // fixed hint line above the composer keeps the command's identity —
  // 「/cmd — description · Enter sends」— instead of the context silently
  // evaporating. Honesty contract: only a REAL hit lights it (unknown
  // `/tokens` — usually pasted paths — render nothing). Pure UI: the send
  // path is untouched (bare /cmd still executes locally, /cmd args and
  // unknown tokens still go to the model as text). Computed inline on
  // purpose — a regex plus a ≤17-row lookup, cheaper than the memo bookkeeping.
  const slashArgs = (() => {
    if (slashQuery || !value.startsWith('/')) return null
    const token = slashArgsToken(value)
    if (!token) return null
    const command = findSlashCommand(token)
    if (command) {
      return { trigger: `/${command.name}`, description: t(command.descriptionKey), icon: command.icon }
    }
    const needle = token.toLowerCase()
    const skill = skills.find(s => s.name.toLowerCase() === needle || s.trigger.toLowerCase() === `/${needle}`)
    if (skill) {
      return { trigger: skill.trigger, description: skill.description, icon: 'auto_fix' }
    }
    return null
  })()

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

  // P2-3: the id the textarea's aria-controls points at — present exactly
  // while the owning listbox is rendered (slash wins; the menus are
  // mutually exclusive, but the order makes that explicit rather than
  // load-bearing).
  const openListboxId = slashOpen
    ? slashListboxId
    : mentionMatches.length > 0
      ? mentionListboxId
      : undefined

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

  // R2 audit G2/I1 (02c A2 可选加分) — Shift+Tab walks the approval ladder
  // (ask → auto-edit → plan → full-auto, wrapping), the Claude Code /
  // Gemini CLI muscle-memory key; the TUI side of the naming design already
  // plans the same loop. Same write surface as the pill's dropdown
  // (configure('approval_mode') + refreshConfig); the toast names the NEW
  // tier so the shortcut is never silent. An out-of-ladder current value
  // (expert modes, engine aliases) enters the loop at `ask` — the managed
  // base — instead of masquerading as a step it isn't.
  const cycleApprovalMode = async () => {
    const current = normalizeApprovalMode(
      (config as Record<string, unknown> | undefined)?.approval_mode as string | undefined ?? '',
    )
    const idx = APPROVAL_MODE_CYCLE.indexOf(current)
    const next = APPROVAL_MODE_CYCLE[(idx + 1) % APPROVAL_MODE_CYCLE.length]
    const nextOption = approvalModeOption(next)
    try {
      await api.configure({ key: 'approval_mode', value: next })
      await refreshConfig()
      toast.success(intl.formatMessage(
        { id: 'chat.input.mode.cycled' },
        { mode: nextOption.rawLabel ?? intl.formatMessage({ id: nextOption.labelKey }) },
      ))
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

  // S3-1 (P-N11 "why is this model active"): one derivation feeds every
  // row's why-label — session override > phase tier > profile-pinned/global
  // default, the same chain `resolve_client_config_for_session` walks at
  // query time. `config` carries the tier prefs + approval mode; the
  // session override arrives from `getSessionModel` above.
  const whyCtx: ModelWhyContext = {
    override: sessionOverride,
    approvalMode: (config as Record<string, unknown> | undefined)?.approval_mode as string | undefined ?? null,
    planTier: (config as Record<string, unknown> | undefined)?.plan_tier as string | undefined ?? null,
    actTier: (config as Record<string, unknown> | undefined)?.act_tier as string | undefined ?? null,
    globalModel: status?.model ?? null,
    activeProfile: status?.active_profile ?? null,
  }
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
        // No session context (tests / degraded catalogs) — legacy global
        // write, funneled through the shared model+provider pair helper
        // (S3-1 P-N23: one write convention on every surface).
        await writeGlobalModelDefault(model)
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

  // S3-5 (P2-19) — effort as a picker sub-tier. The engine's dial has four
  // canonical levels (`low|standard|high|max`, `EffortLevel` in
  // shannon-core); `medium` is a legacy alias of `standard` the write path
  // normalizes away, and a stale persisted `medium` (old demo writes)
  // normalizes here so the highlight never breaks. `standard` is the
  // default — the engine sends nothing at that level.
  const EFFORT_LEVELS = ['low', 'standard', 'high', 'max'] as const
  type EffortValue = (typeof EFFORT_LEVELS)[number]
  const normalizeEffort = (raw: string | null | undefined): EffortValue => {
    const v = (raw ?? '').trim().toLowerCase()
    if (v === 'medium') return 'standard'
    return (EFFORT_LEVELS as readonly string[]).includes(v) ? (v as EffortValue) : 'standard'
  }
  const currentEffort = normalizeEffort(
    (config as Record<string, unknown> | undefined)?.effort_level as string | undefined,
  )
  // R2 audit W5 (02c S2): the gauge menu's rows wear the design's sub-
  // labeled copy — 轻 · 最低延迟 / 标准（默认）/ 深 · 复杂推理 / 最高 ·
  // 极限推理 (chat.input.effort.desc.*). The short words stay on the chip
  // (`chat.input.effort.*`, also the model picker's sub-tier — 02c keeps
  // that surface's current form) and in the aria label.
  const effortOptions: { value: EffortValue; labelKey: string; descKey: string }[] = [
    { value: 'low', labelKey: 'chat.input.effort.low', descKey: 'chat.input.effort.desc.low' },
    { value: 'standard', labelKey: 'chat.input.effort.standard', descKey: 'chat.input.effort.desc.standard' },
    { value: 'high', labelKey: 'chat.input.effort.high', descKey: 'chat.input.effort.desc.high' },
    { value: 'max', labelKey: 'chat.input.effort.max', descKey: 'chat.input.effort.desc.max' },
  ]
  const currentEffortLabel = effortOptions.find(e => e.value === currentEffort)
    ? t(`chat.input.effort.${currentEffort}`)
    : currentEffort
  const handleEffortChange = async (effort: string | null) => {
    if (!effort) return
    try {
      await api.configure({ key: 'effort_level', value: effort })
      await refreshConfig()
    } catch (err) {
      toastError(t('chat.input.effort.failed'), err)
    }
  }

  // 02c 裁决 D4 — the context-% chip's number. Same math the ChatStatusBar
  // segment used: sent+received tokens over the ENGINE-RESOLVED window
  // (`context_total`), clamped, integer percent. Absent window or zero
  // tokens → null → the chip renders icon-only (never a guessed window).
  const contextPct = (() => {
    const total = (usage?.input_tokens ?? 0) + (usage?.output_tokens ?? 0)
    const max = usage?.context_total
    if (!max || total <= 0) return null
    return Math.min(100, Math.round((total / max) * 100))
  })()

  // S3-6 — pre-send estimate. The backend counts with the SAME
  // estimator the engine/billing stack uses (同源 DoD); this side only
  // renders. Advisory by contract: a failed call silently hides the row
  // (never a toast, never a send gate), an empty composer hides it too.
  // Debounced — see ESTIMATE_DEBOUNCE_MS.
  const [costEstimate, setCostEstimate] = useState<SendCostEstimate | null>(null)
  const attachmentsKey = attachedFiles.join('\n')
  useEffect(() => {
    if (!value.trim() && attachedFiles.length === 0) {
      setCostEstimate(null)
      return
    }
    let cancelled = false
    const timer = window.setTimeout(() => {
      api.estimateSendCost(sessionId ?? null, value, attachedFiles)
        .then(est => { if (!cancelled) setCostEstimate(est) })
        .catch(() => {
          // Advisory: estimate unavailability (mock backend, older backend)
          // must never surface as an error — the row just stays hidden.
          if (!cancelled) setCostEstimate(null)
        })
    }, ESTIMATE_DEBOUNCE_MS)
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- attachmentsKey IS the attachedFiles identity (joined): a content-change re-arms the timer, a parent re-render that passes an equal-content array must not.
  }, [value, attachmentsKey, sessionId])
  // Budget linkage: the projected high end against the session cap. The
  // BudgetBanner keeps owning banner duty — this only tints the estimate row.
  // Unknown pricing (costHigh null) leaves the tint off — no fabricated
  // figure feeds it (G3: the row itself no longer shows $ at all).
  const estimateBudgetRatio =
    costEstimate && costEstimate.costHigh != null && costEstimate.budgetUsd && costEstimate.budgetUsd > 0
      ? (costEstimate.spentUsd + costEstimate.costHigh) / costEstimate.budgetUsd
      : null
  const estimateBudgetNearing =
    estimateBudgetRatio !== null && estimateBudgetRatio >= ESTIMATE_BUDGET_WARN_RATIO

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
  // H2 P1-4 — latest-ref mirror for the long-text-paste Undo action. The
  // toast outlives the render that posted it, so its undo closure must read
  // the CURRENT attachment set (same protocol as mergePathsRef above), not
  // the one captured at paste time.
  const attachedFilesRef = useRef(attachedFiles)
  useEffect(() => {
    attachedFilesRef.current = attachedFiles
  })

  // H2 P1-4 — long-text paste → attachment. At or over
  // MAX_INLINE_PASTE_CHARS the text is persisted as a UTF-8 .txt in the same
  // pasted cache the image path uses (`save_pasted_text`, the sibling of
  // save_pasted_image) and rides the regular attachment pipeline (absolute
  // path → preflight → read at send); the textarea receives NOTHING — an
  // 8 KiB dump is reference material, not an editable draft. The sonner
  // toast carries an explicit「撤销」action as the inverse: put the text
  // back into the composer and drop the chip. Below the threshold the paste
  // is never intercepted.
  const handlePastedLongText = async (text: string) => {
    let savedPath: string
    try {
      savedPath = await api.savePastedText(text)
    } catch (err) {
      toastError(t('chat.input.paste.failed'), err)
      return
    }
    mergePathsRef.current([savedPath])
    const undo = () => {
      // onAttach is REPLACE semantics (see mergePaths): hand back the
      // current set minus the converted paste. The latest-ref read keeps a
      // late click honest after further attaches; a click landing before
      // the parent flushes the attach still wins — the last set replaces
      // the pending union.
      onAttach(attachedFilesRef.current.filter(p => p !== savedPath))
      // Restore the text — appended after anything typed since (blank-line
      // separated, the same shape as the composer-draft bridge), or alone
      // when the composer is empty. Caret parked at the end.
      const el = textareaRef.current
      const current = el?.value ?? ''
      const restored = current.trim() ? `${current.replace(/\s+$/, '')}\n\n${text}` : text
      onChange(restored)
      requestAnimationFrame(() => {
        el?.focus()
        el?.setSelectionRange(restored.length, restored.length)
      })
    }
    // 10s: reading/deciding over an 8 KiB paste takes longer than the
    // default toast window, and the undo affordance must outlive it.
    toast.success(t('chat.input.paste.longText.toast'), {
      action: { label: t('chat.input.paste.longText.undo'), onClick: undo },
      duration: 10000,
    })
  }
  //
  // P2-2 — mixed image+text pastes (Excel ranges, "copy image with
  // caption") used to swallow the text: the handler preventDefault'd the
  // whole event the moment an image was present. The text is only kept by
  // the browser's default insertion, so that default stays live whenever
  // the clipboard also carries string items; we never insert text
  // ourselves, so there is no double-insert path. Image-only pastes (the
  // screenshot-tool case) still preventDefault — nothing else on the
  // clipboard would be lost.
  const handlePaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const images = imageFilesFromClipboard(e.clipboardData)
    if (images.length > 0) {
      if (!clipboardHasText(e.clipboardData)) e.preventDefault()
      void handlePastedImages(images)
      return
    }
    // H2 P1-4 — no image on the clipboard: a very long TEXT paste converts
    // to a .txt attachment (see handlePastedLongText) and cancels the
    // default insertion; everything else falls through untouched — the
    // default text insertion (and IME composition) behavior is never
    // intercepted. (A paste carries no IME composition, so this branch
    // stays clear of the composition guard by construction.)
    const text = e.clipboardData.getData('text/plain')
    if (text.length >= MAX_INLINE_PASTE_CHARS) {
      e.preventDefault()
      void handlePastedLongText(text)
    }
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
    // G2/I1 — Shift+Tab cycles the approval ladder (ask → auto-edit → plan
    // → full-auto). Deliberately AFTER the mention/slash blocks: while a
    // menu is open, Tab — shift or not — belongs to that menu's commit path
    // above, so the cycle only ever fires with BOTH menus closed.
    if (e.key === 'Tab' && e.shiftKey) {
      e.preventDefault()
      void cycleApprovalMode()
      return
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

  const currentMode = config?.approval_mode || 'auto-edit'
  const planModeActive = currentMode === 'plan'
  // GB P2-4: the pill reads the SHARED five-tier table (same source as
  // Settings → General). Values outside the table (engine-only aliases)
  // render honestly via the fallback instead of masquerading as Suggest.
  // 02c 裁决 D3: ONE form at every width — the active-mode chip + dropdown
  // (the old segmented/breakpoint split is gone).
  const selectedMode = approvalModeOption(currentMode)

  // The composer owns ONE mode surface (the unified pill below); the
  // keyboard shortcut and the plan banner's exit button both funnel here.
  // (The old design had a separate 计划模式 toggle button alongside the
  // approval-mode select that also contained 计划/只读 — two controls
  // writing the same `approval_mode` key, which read as overlapping modes.)
  const handlePlanToggle = async () => {
    try {
      await api.configure({ key: 'approval_mode', value: planModeActive ? 'ask' : 'plan' })
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
  // R2 audit W6 (02c S1): the menu rows use the design copy —「添加文件…」
  // (the shared attach key, single-surface) and a composer-specific「PPT
  // 大纲…」(office.ppt.title stays the dialog's own title, it is reused
  // there), and a separator groups the editor tools apart from the
  // low-frequency 临时会话 toggle at the bottom.
  const plusItems: DropdownMenuEntry[] = [
    { id: 'attach', label: t('chat.input.attach.aria'), icon: 'attach_file', onSelect: () => { setPlusOpen(false); void handleAttachClick() } },
    { id: 'ppt', label: t('chat.input.plus.ppt'), icon: 'slideshow', onSelect: () => { setPlusOpen(false); setPptOpen(true) } },
    { id: 'quickfix', label: t('nav.quickFix'), icon: 'build', onSelect: () => { setPlusOpen(false); onOpenQuickFix() } },
    { id: 'editor', label: t('nav.editor'), icon: 'code', onSelect: () => { setPlusOpen(false); onOpenEditor() } },
    DropdownMenuSeparator('plus-editor-sep'),
    // 02c 裁决 D4(菜单扩容): the low-frequency memory-bypass toggle folds
    // into the + menu — the state rides the label (开/关) and the trigger
    // wears a dot while active; the banner above the input keeps spelling
    // the active state out. Same testid as the retired standalone button.
    { id: 'memory-bypass', testid: 'memory-bypass-toggle', label: memoryBypassed
        ? t('chat.input.memoryBypass.menu.on')
        : t('chat.input.memoryBypass.menu.off'), icon: 'psychology', onSelect: () => { setPlusOpen(false); void handleMemoryBypassToggle() } },
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
      {/* H2 P1-2 — the "/cmd args" hint line. Same slot as the slash menu
          (the two are regex-exclusive) but deliberately lighter: one quiet
          row, not a listbox and not a message card. No live region — it
          re-renders per keystroke and would repeat the P2-9 announcement
          mistake; the content is plain text read in place. */}
      {slashArgs && (
        <div
          role="note"
          data-testid="slash-args-hint"
          className="absolute left-0 right-0 bottom-full mb-sm z-modal flex items-center gap-sm rounded-xl border border-outline-variant/30 bg-surface-container-low px-md py-xs shadow-e3"
        >
          <span className="material-symbols-outlined icon-sm text-primary shrink-0">{slashArgs.icon}</span>
          <span className="font-mono text-label-md text-on-surface shrink-0">{slashArgs.trigger}</span>
          <span className="font-label-sm text-on-surface-variant truncate flex-1">{slashArgs.description}</span>
          <span className="font-label-xs text-on-surface-variant whitespace-nowrap shrink-0">{t('slash.argsHint.send')}</span>
        </div>
      )}

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
        <div className="absolute inset-0 z-raised flex items-center justify-center bg-primary-container rounded-2xl backdrop-blur-sm pointer-events-none">
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
        <div className="flex items-center justify-center py-sm bg-primary-container rounded-t-2xl">
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
            <div className="font-label-xs text-warning/80 mt-[1px]">
              {t('chat.attach.reason.noWorkingDir')}
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            data-testid="no-working-dir-open-settings"
            onClick={() => navigate('/settings')}
            className="rounded-sm hover:bg-warning/10 shrink-0"
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
            <div className="font-label-xs text-warning/80 mt-[1px]">
              {t('chat.input.attach.unsupportedHint')}
            </div>
          </div>
          <Button
            variant="ghost"
            size="icon-xs"
            onClick={() => setUnparsedDismissed(true)}
            aria-label={t('chat.message.attachment.close')}
            title={t('chat.message.attachment.close')}
            className="rounded-sm hover:bg-warning/10 shrink-0"
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

        {/* G1-A (2026-10-15 review §1.2 chat): no sparkle icon and no session
            name in the placeholder row — a plain textarea whose placeholder
            (queued variant while streaming) reads at the design's composer
            size/weight: body-sm 14px text, t3-muted placeholder. */}
        <div className="flex items-start px-sm">
          <textarea
            ref={textareaRef}
            className="flex-1 bg-transparent border-none outline-none focus:ring-0 font-body-sm py-md px-sm placeholder:text-muted-foreground text-on-surface resize-none min-h-[24px] max-h-[200px]"
            placeholder={
              isQuerying
                ? t('chat.input.queued.placeholder')
                : t('chat.input.placeholder.empty')
            }
            aria-label={t('chat.input.ariaLabel')}
            // P2-3: names the open menu's listbox (absent when both are
            // closed); selection/announcement flow through the P2-9 status
            // regions, not aria-activedescendant.
            aria-controls={openListboxId}
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

        {/* S3-6 / G3 (2026-10-15 parity review §2.3-2) — pre-send estimate
            (display-only, never a send gate). One quiet line under the draft:
            「≈ N tokens」— the projected INPUT the user can actually sanity-
            check before sending (the Chatbox-validated form). The $ range is
            gone from this row: cost belongs to the audit surfaces, a token
            count needs no rate table and stays honest for unknown-rate
            models. At ≥80% of the session budget the row still turns
            warning-toned and names the share — copy linkage only;
            BudgetBanner remains the single banner. Purely visual (no
            aria-live): per-keystroke announcements would repeat the P2-9
            mistake. */}
        {costEstimate && (
          <div
            data-testid="send-cost-estimate"
            title={t('chat.input.cost.aria')}
            className={cn(
              'flex items-center justify-end gap-xs px-sm pt-xs font-label-xs tabular-nums',
              estimateBudgetNearing ? 'text-warning' : 'text-on-surface-variant/70',
            )}
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">token</span>
            <span>
              {intl.formatMessage({ id: 'chat.input.estimate.tokens' }, {
                count: new Intl.NumberFormat(intl.locale).format(costEstimate.inputTokens),
              })}
            </span>
            {estimateBudgetNearing && estimateBudgetRatio !== null && (
              <span data-testid="send-cost-budget-note" className="font-bold">
                {intl.formatMessage(
                  { id: 'chat.input.cost.budget' },
                  { percent: Math.round(estimateBudgetRatio * 100) },
                )}
              </span>
            )}
          </div>
        )}

        <div className="flex items-center justify-between gap-xs px-sm py-xs border-t border-outline-variant/20">
          {/* 02c 收敛后的 composer 行(裁决 D1-D5): one "+" menu, the
              active-mode chip (dropdown ladder), the context-% chip, the
              model chip and the effort gauge on the left; mic / counter /
              send on the right. 状态条数字与规划/执行档位已按裁决移除。 */}
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
              {/* G1-A (2026-10-15 review §1.2 chat): the design's attach-dot —
                  a 6px aurora-cyan bead on the ⊕'s top-right corner
                  (shannon-ui.css .attach-dot, --aur). The temporary-chat dot
                  shares the slot: the bypass state is the more urgent signal,
                  so it takes the bead over while active (its banner above the
                  input still spells the full state out). */}
              <span
                aria-hidden="true"
                data-testid={memoryBypassed ? 'composer-bypass-dot' : 'composer-attach-dot'}
                className={cn(
                  'absolute right-1 top-1 size-1.5 rounded-full',
                  memoryBypassed ? 'bg-secondary' : 'bg-[var(--aurora-cyan)]',
                )}
              />
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

            {/* 02c 裁决 D3(二次修正): the approval ladder is a SINGLE chip
                showing the ACTIVE mode, opening the four-stop dropdown (逐档
                说明 + high-risk note) — the segmented control and its 1200px
                breakpoint are gone; one form at every width. The testid stays
                `approval-mode-pill`. Out-of-ladder engine values (expert
                modes, CLI aliases) never masquerade: the raw value echoes in
                the trigger via SelectValue's rawLabel fallback. */}
            <Select value={selectedMode.value} onValueChange={handleModeChange}>
              <SelectTrigger
                size="sm"
                data-testid="approval-mode-pill"
                aria-label={t('chat.input.mode.label')}
                title={`${selectedMode.rawLabel ? selectedMode.rawLabel : t(selectedMode.descriptionKey)} · ${t('chat.input.mode.highRiskNote')}`}
                // G1-A S1 chip geometry (28px / radius 8 / 12px label) from the
                // .chip utility; the risk-ladder border tone (neutral hairline
                // ask — the 2026-10-17 裁决 dropped its amber, the supervised
                // base tier is the default state, not a warning — amber
                // auto-edit, red full-auto, green plan) is design r4 §E.4 as
                // amended and stays. fill stays transparent (quiet chip).
                className={cn('chip border', selectedMode.tone, 'bg-transparent hover:bg-surface-container-low/50 transition-colors')}
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
              {/* The composer sits on the window's bottom edge, and the
                  default trigger-aligned popup opens DOWNWARD from the
                  selected item — the ladder + high-risk note then overflow
                  the viewport and the popup scrolls internally (bug:
                  权限模式弹出框显示不全面). Open upward with standard
                  popover positioning instead: the whole ladder and the note
                  fit in the ~550px between the composer and the header.
                  min-w-56 stops the two-line descriptions wrapping three
                  deep (the trigger-aligned w-(--anchor-width) was pill-width). */}
              <SelectContent side="top" alignItemWithTrigger={false} className="min-w-56">
                {[...APPROVAL_MODES, PLAN_OPTION].map(mode => (
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

            {/* 02c 裁决 D4 — 会话用量入口保留在模型 chip 旁,升级为上下文
                百分比 chip:引擎已解析窗口时显示 icon + N%,否则只有 icon
                (诚实契约——绝不猜窗口)。点击打开会话用量弹框,状态条的
                上下文/缓存/花费/预算数字全部收编其中。弹框点击后才挂载,
                composer 首屏零开销。 */}
            <Button
              variant="ghost"
              aria-haspopup="dialog"
              aria-expanded={usageOpen}
              data-testid="usage-chip"
              aria-label={t('chat.input.usage.aria')}
              title={t('chat.input.usage.title')}
              // G1-A S1 chip geometry; with a resolved window the chip wears
              // the quiet token chrome (≈ white 7% border / 4% fill on dark),
              // icon-only stays borderless (the honesty contract is untouched).
              className={cn(
                'chip border',
                contextPct != null
                  ? 'border-outline-variant/40 bg-surface-container/30 text-on-surface-variant hover:text-primary hover:bg-surface-container-low/50'
                  : 'border-transparent bg-transparent text-on-surface-variant hover:text-primary',
              )}
              onClick={() => setUsageOpen(true)}
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">data_usage</span>
              {contextPct != null && (
                <span className="tabular-nums">{contextPct}%</span>
              )}
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
                  ? `${intl.formatMessage(
                      { id: 'chat.input.model.sessionTitle' },
                      { model: currentModel?.name ?? sessionOverride.model },
                    )} ${t('chat.input.model.sessionPinNote')}`
                  : t('chat.input.model.title')}
                // G1-A S1 chip geometry + quiet token chrome.
                className="chip max-w-[170px] border border-outline-variant/40 bg-surface-container/30 text-on-surface-variant hover:bg-surface-container-low/50 transition-colors"
              >
                <span className="material-symbols-outlined icon-sm">smart_toy</span>
                {/* Render the effective model NAME — the session override's
                    model when one is active (with a "· session" suffix so the
                    override is never silent), else the global default. S3-5
                    (P2-19): the reasoning effort is NO LONGER glued onto this
                    label (`name · High` is gone) — it wears its own badge
                    beside the chip below, and its interactive home is the
                    picker's effort sub-tier. */}
                <SelectValue placeholder={status?.model || t('chat.input.model.label')}>
                  {(value: unknown) => {
                    const m = modelList.find(x => x.id === value)
                    const name = m?.name
                      ?? sessionOverride?.model
                      ?? status?.model
                      ?? t('chat.input.model.label')
                    return sessionOverride
                      ? `${name} · ${t('chat.input.model.sessionSuffix')}`
                      : name
                  }}
                </SelectValue>
              </SelectTrigger>
              {/* R2-3: widened past the chip's anchor width so the context/
                  price meta fits on the model rows; S3-1 widens further —
                  the rows now also carry the source badge + why-active
                  label (shared ModelPickerRowContent). */}
              <SelectContent className="w-[380px]">
                {/* S3-1 (P-N11): the priority chain, stated where the labels
                    below are worn — Claude Code's picker states its lookup
                    order instead of leaving it implicit. */}
                {modelList.length > 0 && (
                  <div
                    role="presentation"
                    data-testid="model-picker-priority-line"
                    className="px-sm pt-0 pb-xs font-label-xs text-on-surface-variant"
                  >
                    {t('chat.input.model.priorityLine')}
                  </div>
                )}
                {/* S3-5 (裁定⑪/P2-19): the effort dial is a SECOND-LEVEL
                    interaction inside the picker — the effective model's row
                    expands its effort sub-tier directly beneath it (the
                    Codex /model shape). Not a separate control (that would
                    add a switching surface, against S3-1's convergence), and
                    no longer a bottom section disconnected from any model. */}
                {modelList.flatMap(m => {
                  const row = (
                    <SelectItem key={m.id} value={m.id} data-testid={`model-option-${m.id}`}>
                      <ModelPickerRowContent model={m} why={modelWhyFor(m, whyCtx, modelList)} />
                    </SelectItem>
                  )
                  if (m.id !== (currentModel?.id ?? '')) return [row]
                  return [
                    row,
                    <Fragment key={`effort-subtier-${m.id}`}>
                      <div
                        role="presentation"
                        data-testid="effort-subtier-header"
                        className="px-sm pt-xs pb-0 font-label-2xs uppercase tracking-wider text-on-surface-variant"
                      >
                        {t('chat.input.effort.section')}
                      </div>
                      {/* Three-state honesty (mirrors the engine): the engine
                          passes effort parameters through and the PROVIDER
                          arbitrates, so a known-no-reasoning model gets a note
                          — not a lockout. Unknown (`undefined`/null) renders
                          normally. */}
                      {currentModel?.reasoning === false && (
                        <div
                          role="note"
                          data-testid="effort-not-applicable-note"
                          className="px-sm pb-xs text-label-2xs text-on-surface-variant"
                        >
                          {t('chat.input.effort.notApplicable')}
                        </div>
                      )}
                      {effortOptions.map(effort => (
                        <SelectItem
                          key={`effort-${effort.value}`}
                          value={`effort:${effort.value}`}
                          data-testid={`effort-option-${effort.value}`}
                        >
                          {/* Indented under the model row: the sub-tier
                              reading of 裁定⑪, kept inside the Select's own
                              item semantics (keyboard + SR for free). */}
                          <span className="flex items-center gap-xs pl-md">
                            <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                              {currentEffort === effort.value ? 'radio_button_checked' : 'radio_button_unchecked'}
                            </span>
                            <span className={currentEffort === effort.value ? 'font-medium' : undefined}>
                              {t(effort.labelKey)}
                            </span>
                          </span>
                        </SelectItem>
                      ))}
                    </Fragment>,
                  ]
                })}
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
                    {/* S1-2 (P-N2): the pin's failover contract, stated where
                        the pin action lives — a pinned session opts THIS chat
                        out of model-level automatic failover (key rotation is
                        unaffected). */}
                    <div role="presentation" className="px-sm pt-0 pb-xs text-label-xs text-on-surface-variant">
                      {t('chat.input.model.sessionPinNote')}
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
              </SelectContent>
            </Select>
            {/* 02c 裁决 D2 — the effort dial is a CLICKABLE gauge chip beside
                the model chip (the ChatGPT/Codex-desktop "compact composer
                gauge"): four dots + the current level word, opening the
                four-level menu in one click. The model picker's effort
                sub-tier above stays — both surfaces write the same
                configure('effort_level'). At the `standard` default the chip
                rests dim (text + dots de-emphasized) so the row stays quiet;
                a non-default level lights it up. testid: effort-gauge;
                options carry effort-gauge-option-* (the picker's
                effort-option-* ids are untouched). */}
            <Menu.Root modal={false}>
              <Menu.Trigger
                render={
                  <button
                    type="button"
                    data-testid="effort-gauge"
                    aria-label={intl.formatMessage({ id: 'chat.input.effort.gauge.aria' }, { label: currentEffortLabel })}
                    title={t('chat.input.effort.title')}
                    // G1-A S1 chip geometry. Active (non-standard) wears the
                    // vio-dim container pair + violet border (the design's
                    // 激活态紫); the standard default rests dim/quiet.
                    className={cn(
                      'chip shrink-0 border transition-colors cursor-pointer',
                      currentEffort !== 'standard'
                        ? 'border-primary/40 bg-primary-container text-on-primary-container hover:border-primary/60'
                        : 'border-outline-variant/40 bg-surface-container/30 text-on-surface-variant hover:text-primary hover:bg-surface-container-low/50',
                    )}
                  >
                    <span className="material-symbols-outlined icon-sm" aria-hidden="true">speed</span>
                    <span className="flex items-center gap-[2.5px] max-[1199px]:hidden" aria-hidden="true">
                      {effortOptions.map(e => (
                        <span
                          key={e.value}
                          className={cn(
                            'size-[5px] rounded-full',
                            effortOptions.findIndex(x => x.value === currentEffort) >= effortOptions.findIndex(x => x.value === e.value)
                              // G1-A: lit dots go VIOLET (--color-primary) on the
                              // active gauge; the standard default keeps its
                              // low-contrast dim state.
                              ? currentEffort !== 'standard' ? 'bg-primary' : 'bg-on-surface-variant/50'
                              : 'bg-on-surface-variant/20',
                          )}
                        />
                      ))}
                    </span>
                    {currentEffortLabel}
                    <span className="material-symbols-outlined icon-sm opacity-60" aria-hidden="true">expand_more</span>
                  </button>
                }
              />
              <Menu.Portal>
                {/* z-modal rides the POSITIONER (same token scale as the
                    composer's other floating surfaces). */}
                <Menu.Positioner align="start" side="top" sideOffset={8} className="isolate z-modal">
                  <Menu.Popup
                    role="dialog"
                    aria-labelledby={undefined}
                    aria-label={t('chat.input.effort.label')}
                    data-testid="effort-gauge-menu"
                    className="glass-overlay animate-panel-in w-[248px] rounded-xl py-sm outline-none"
                  >
                    <div className="px-md pt-xs pb-sm">
                      <div className="font-headline-sm text-on-surface font-bold">{t('chat.input.effort.label')}</div>
                      <div className="text-label-sm text-on-surface-variant">{t('chat.input.effort.title')}</div>
                    </div>
                    <Menu.RadioGroup
                      value={currentEffort}
                      onValueChange={v => { void handleEffortChange(v as string | null) }}
                      className="px-md pb-xs"
                    >
                      {effortOptions.map(effort => (
                        <Menu.RadioItem
                          key={effort.value}
                          value={effort.value}
                          data-testid={`effort-gauge-option-${effort.value}`}
                          label={t(effort.descKey)}
                          className="flex items-center gap-xs rounded-lg px-md py-xs font-label-md cursor-pointer outline-none text-on-surface-variant data-[highlighted]:bg-primary-container data-[highlighted]:text-on-surface"
                        >
                          <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                            {currentEffort === effort.value ? 'radio_button_checked' : 'radio_button_unchecked'}
                          </span>
                          <span className={currentEffort === effort.value ? 'font-medium text-on-surface' : undefined}>
                            {t(effort.descKey)}
                          </span>
                        </Menu.RadioItem>
                      ))}
                    </Menu.RadioGroup>
                    {currentModel?.reasoning === false && (
                      /* Three-state honesty (mirrors the picker's sub-tier
                          note): the engine passes effort through and the
                          PROVIDER arbitrates — a known-no-reasoning model
                          gets the note, not a lockout. */
                      <div
                        role="note"
                        data-testid="effort-gauge-not-applicable"
                        className="mx-md mb-sm border-t border-outline-variant/20 pt-xs text-label-2xs text-on-surface-variant"
                      >
                        {t('chat.input.effort.notApplicable')}
                      </div>
                    )}
                    {/* R2 audit W5 (02c S2 cnote): the ALWAYS-on footer
                        note — 力度随会话生效；标准时 gauge 低对比显示. It
                        coexists with the conditional honesty line above
                        (which only renders for known-no-reasoning models)
                        and stays last, at the menu's bottom. */}
                    <div
                      role="note"
                      data-testid="effort-gauge-note"
                      className="mx-md mb-sm border-t border-outline-variant/20 pt-xs text-label-2xs text-on-surface-variant"
                    >
                      {t('chat.input.effort.note')}
                    </div>
                  </Menu.Popup>
                </Menu.Positioner>
              </Menu.Portal>
            </Menu.Root>
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
                  className={cn('font-mono text-label-xs tabular-nums px-xs', isOverSoftWarn ? 'text-error' : 'text-on-surface-variant')}
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
                    // G1-A: the send-slot CTA geometry (design .send: 32px /
                    // radius 10 / --vio-deep fill, white glyph; hover lifts to
                    // the light violet --vio).
                    className="bg-[var(--vio-deep)] text-white size-8 rounded-[10px] active:scale-95 hover:bg-[var(--vio)] transition-all"
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
                  className="bg-error-container text-on-error p-3 rounded-xl active:scale-95 transition-all disabled:opacity-60 disabled:cursor-wait"
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
                // G1-A (2026-10-15 review S4): 32px / radius 10 / solid
                // --vio-deep with the white arrow (design .send); hover lifts
                // to the light violet --vio (only a hover color in the design
                // system). Disabled keeps the base cva's gray fill.
                className="bg-[var(--vio-deep)] text-white size-8 rounded-[10px] active:scale-95 hover:bg-[var(--vio)] transition-all disabled:opacity-40 disabled:cursor-not-allowed"
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
