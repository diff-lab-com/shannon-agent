// Barrel for Chat page sub-components. Keeps the orchestrator's import block
// short and centralizes the seam if any of these need to be relocated.

export { default as ApiKeyBanner } from './ApiKeyBanner'
export { default as ChatSearchBar } from './ChatSearchBar'
export { default as ComposerPanel } from './ComposerPanel'
export { ComposerContext, useComposer } from './ComposerContext'
// B4 audit: the ContextPanel default export (dead slide-in wrapper) was
// removed; `ContextPanelContent` is imported directly by RightDock.
export { default as InlinePanelModal } from './InlinePanelModal'
export { default as MessageArea, VIRTUALIZE_THRESHOLD } from './MessageArea'
export { default as QueueChips } from './QueueChips'
export { formatDirBreadcrumb } from './utils'
// Session actions moved to lib/ (U1) so the app-sidebar session rail can use
// them without importing a page module.
export { changeSessionWorkingDir, exportSessionAsMarkdown, printSession } from '@/lib/sessionActions'
