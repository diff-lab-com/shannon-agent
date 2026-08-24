import { Button } from '@/components/ui/button'
import { formatDirBreadcrumb } from './utils'

interface ChatHeaderProps {
  t: (id: string) => string
  currentSessionTitle: string
  fallbackTitle: string
  sessionWorkingDir: string
  contextPanelOpen: boolean
  toggleContextPanel: () => void
  handleChangeWorkingDir: () => Promise<void>
}

export default function ChatHeader({
  t,
  currentSessionTitle,
  fallbackTitle,
  sessionWorkingDir,
  contextPanelOpen,
  toggleContextPanel,
  handleChangeWorkingDir,
}: ChatHeaderProps) {
  return (
    <header
      role="banner"
      aria-label={t('chat.header.aria')}
      className="relative shrink-0 flex items-center gap-md px-lg py-sm bg-surface/60 backdrop-blur-sm border-b border-outline-variant/15"
    >
      <div className="flex items-center gap-sm min-w-0 flex-1">
        <span className="material-symbols-outlined text-primary text-[20px] shrink-0">forum</span>
        <div className="min-w-0 flex-1">
          <h2 className="font-headline-sm font-bold text-on-surface truncate leading-tight">
            {currentSessionTitle || fallbackTitle || t('chat.empty.start')}
          </h2>
          {sessionWorkingDir && (
            <Button
              type="button"
              variant="ghost"
              onClick={handleChangeWorkingDir}
              className="mt-[2px] flex items-center gap-xs text-label-sm text-on-surface-variant hover:text-primary transition-colors max-w-full"
              title={sessionWorkingDir}
            >
              <span className="material-symbols-outlined text-[14px] opacity-70 shrink-0">folder</span>
              <span className="truncate font-mono">{formatDirBreadcrumb(sessionWorkingDir)}</span>
            </Button>
          )}
        </div>
      </div>
      <Button
        type="button"
        variant="ghost"
        onClick={toggleContextPanel}
        className="p-xs rounded-lg text-on-surface-variant hover:text-primary hover:bg-surface-container focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none shrink-0"
        title={t('chat.header.contextPanel.toggle')}
        aria-label={t('chat.header.contextPanel.toggle')}
        aria-expanded={contextPanelOpen}
        aria-pressed={contextPanelOpen}
      >
        <span className="material-symbols-outlined icon-md">{contextPanelOpen ? 'right_panel_close' : 'right_panel_open'}</span>
      </Button>
    </header>
  )
}