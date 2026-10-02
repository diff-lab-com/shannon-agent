// P1.5 (chat-testing plan v2 §9.5) — central data-testid registry for the
// chat domain. Single source of truth against string drift: components keep
// their inline `data-testid` literals, and this table mirrors them so specs
// anchor via `getByTestId(testids.x)` instead of hand-typed selectors. Any
// NEW anchor must be registered here before a spec may reference it.
//
// Not imported by components (they own the literal); vitest may import it
// relative-path style, same contract as helpers/a11yDebt.ts. Templated ids
// (per-entity suffixes) live in `testidTemplates` below — same discipline.
//
// Kept import-free of '@playwright/test' so vitest suites can lock the
// table (see src/__tests__/a11yDebt.test.ts for the precedent).

export const testids = {
  // ── Header (components/Header.tsx) ────────────────────────────────────
  /** Permission alertdialog container (Base UI portal popup). */
  permissionDialog: 'permission-dialog',
  /** Session spent/budget badge (P0-4; rendered while a budget cap is set). */
  budgetBadge: 'budget-badge',
  /** Stop control mounted above the approval scrim mid-run (S-3 fix). */
  headerStopWhileWaiting: 'header-stop-while-waiting',

  // ── Header switchers (components/chat/) ───────────────────────────────
  /** ExecutionModeSwitcher trigger (严格/平衡/宽松/自定义). */
  executionModeSwitcher: 'execution-mode-switcher',
  /** PhaseTierSwitcher trigger (规划/执行档位). */
  phaseTierSwitcher: 'phase-tier-switcher',
  /** PhaseTierSwitcher dropdown menu (role=listbox). */
  phaseTierMenu: 'phase-tier-menu',
  /** Global plan/act tier section on Settings→Models. */
  phaseTierSection: 'phase-tier-section',

  // ── Composer (components/chat/ChatInput.tsx) ──────────────────────────
  /** Model chip trigger button (composer, single model surface on /chat). */
  modelChipTrigger: 'model-chip-trigger',
  /** Model chip "Set as default" action (promotes session model to global). */
  modelActionSetDefault: 'model-action-set-default',
  /** Model chip "Reset to default" action (clears the session override). */
  modelActionClearOverride: 'model-action-clear-override',
  /** Plan-mode status strip above the input (role=status). */
  planModeBanner: 'plan-mode-banner',
  /** Approval-mode pill trigger (SelectTrigger, rounded). */
  approvalModePill: 'approval-mode-pill',
  /** Composer "+" menu trigger (attach / slides / quickfix / editor). */
  composerPlusMenu: 'composer-plus-menu',
  /** Temporary-session (memory bypass) toggle. */
  memoryBypassToggle: 'memory-bypass-toggle',
  /** "This session does not use memory" status strip. */
  memoryBypassBanner: 'memory-bypass-banner',
  /** No-working-dir preflight banner. */
  noWorkingDirBanner: 'no-working-dir-banner',
  /** No-working-dir banner's Settings deep link. */
  noWorkingDirOpenSettings: 'no-working-dir-open-settings',

  // ── Banners / overlays (pages/chat + Chat.tsx) ────────────────────────
  /** ApiKeyBanner root (Banner primitive; class shannon-apikey-banner kept). */
  apikeyBanner: 'apikey-banner',
  /** Auth-failure error banner (provider deep link + Update key). */
  authErrorBanner: 'auth-error-banner',
  /** Session switch overlay (multi-session race guard). */
  sessionSwitchOverlay: 'session-switch-overlay',
  /** Edit-rewind banner. */
  editBanner: 'edit-banner',
  /** Retry-chain banner. */
  retryChainBanner: 'retry-chain-banner',
  /** Delete-session confirmation modal submit. */
  deleteSessionConfirm: 'delete-session-confirm',

  // ── Message area ──────────────────────────────────────────────────────
  /** Screen-reader live region mirroring run status. */
  streamStatusRegion: 'stream-status-region',
  /** Inline run status line under the streaming reply. */
  runStatusLine: 'run-status-line',
  /** Prompt queue (queued turns) panel. */
  promptQueue: 'prompt-queue',
  /** Single queued-turn chip. */
  promptQueueChip: 'prompt-queue-chip',
  /** Right-dock plan panel. */
  planPanel: 'plan-panel',
  /** Right-dock run panel. */
  runPanel: 'run-panel',
  /** Goal start form success strip. */
  goalStartSuccess: 'goal-start-success',

  // ── Search ────────────────────────────────────────────────────────────
  /** In-conversation search bar. */
  chatSearchBar: 'chat-search-bar',
  /** Search match counter ("n / m"). */
  chatSearchCount: 'chat-search-count',

  // ── Tool / artifact cards ─────────────────────────────────────────────
  /** File-changes card (writes/edits summary). */
  fileChangesCard: 'file-changes-card',
  /** Subagent run block. */
  subagentBlock: 'subagent-block',
  /** Inherit-mode hint line inside the subagent block. */
  subagentInheritMode: 'subagent-inherit-mode',
  /** Generated-file card root. */
  fileCard: 'file-card',
  /** File card "Run batch" action. */
  fileCardBatchRun: 'file-card-batch-run',
  /** File card extraction status. */
  fileCardExtraction: 'file-card-extraction',
  /** File card "Review diff" action. */
  fileCardReviewDiff: 'file-card-review-diff',
  /** File card "View extracted" action. */
  fileCardViewExtracted: 'file-card-view-extracted',
  /** Attachment chip parse-issue badge. */
  attachmentChipIssue: 'attachment-chip-issue',
  /** Attachment chip extraction status. */
  attachmentChipExtraction: 'attachment-chip-extraction',
  /** Attachment chip deferred-parse placeholder. */
  attachmentChipDeferred: 'attachment-chip-deferred',
  /** Artifact provenance doc header (right dock). */
  artifactDocHeader: 'artifact-doc-header',
  /** Artifact html-static hint. */
  artifactHtmlStaticHint: 'artifact-html-static-hint',

  // ── Memory / citations ────────────────────────────────────────────────
  /** Injected-memories section (context breakdown). */
  injectedMemories: 'injected-memories',
  /** Source pill root (citation pill under tool output). */
  sourcePill: 'source-pill',
  /** Source pill open action. */
  sourcePillOpen: 'source-pill-open',
  /** Source pill row. */
  sourcePillRow: 'source-pill-row',

  // ── Dialogs spawned from the composer ─────────────────────────────────
  /** PPT outline dialog input. */
  pptOutlineInput: 'ppt-outline-input',
  /** Batch run dialog instruction input. */
  batchInstructionInput: 'batch-instruction-input',
  /** Batch run dialog build prompt action. */
  batchBuildPrompt: 'batch-build-prompt',

  // ── Right dock ────────────────────────────────────────────────────────
  /** Right-dock keyboard shortcut hint. */
  dockShortcutHint: 'dock-shortcut-hint',
} as const

/**
 * Templated testids — one literal pattern, per-entity suffix. Same rule as
 * the static table: build anchors here, never by hand in a spec.
 */
export const testidTemplates = {
  /** Sidebar session row (`desktop-session-row-<sessionId>`). */
  desktopSessionRow: (sessionId: string): string => `desktop-session-row-${sessionId}`,
  /** Model chip dropdown option (`model-option-<modelId>`). */
  modelOption: (modelId: string): string => `model-option-${modelId}`,
  /** Right-dock tab (`dock-tab-<key>`; artifacts use `dock-tab-a-<id>`). */
  dockTab: (key: string): string => `dock-tab-${key}`,
  /** Stream notice line (`stream-notice-<kind>`). */
  streamNotice: (kind: string): string => `stream-notice-${kind}`,
  /** Memory citation pill (`memory-citation-<id>`). */
  memoryCitation: (id: string): string => `memory-citation-${id}`,
  /** Memory citation jump link (`memory-citation-jump-<id>`). */
  memoryCitationJump: (id: string): string => `memory-citation-jump-${id}`,
  /** Injected-memory jump link (`injected-memory-jump-<id>`). */
  injectedMemoryJump: (id: string): string => `injected-memory-jump-${id}`,
} as const
