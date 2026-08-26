import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { TextLoop } from '@/components/reactbits/TextLoop'
import { formatShortcut } from '@/lib/platform'
import { WELCOME_EXAMPLES } from './welcomeExamples'

interface WelcomeStateProps {
  onSelectPrompt: (prompt: string) => void
}

export default function WelcomeState({ onSelectPrompt }: WelcomeStateProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  // Cycled subtitle items. Order mirrors the example-card order below so the
  // highlighted verb ("draft emails") cues the next card the user is likely to
  // reach for. TextLoop honors prefers-reduced-motion (static) and window blur
  // (paused) — see T2.1 guards.
  const loopItems = [
    t('welcomeState.subtitleItem.email'),
    t('welcomeState.subtitleItem.summarize'),
    t('welcomeState.subtitleItem.research'),
    t('welcomeState.subtitleItem.code'),
  ]
  return (
    <div className="flex items-center justify-center h-full min-h-full">
      <div className="text-center max-w-[560px] w-full mx-auto px-lg">
        <div className="w-9 h-9 rounded-full bg-primary-container/40 flex items-center justify-center mx-auto mb-md">
          <span className="material-symbols-outlined icon-md text-primary">auto_awesome</span>
        </div>
        <h2 className="font-headline-md text-headline-md text-on-surface mb-xs">{t('welcomeState.title')}</h2>
        <p className="font-body-md text-on-surface-variant mb-xl">
          {t('welcomeState.subtitlePrefix')}{' '}
          <TextLoop items={loopItems} className="text-primary font-medium" />
        </p>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-sm">
          {WELCOME_EXAMPLES.map(ex => (
            <Button
              key={ex.icon}
              variant="outline"
              className="h-auto justify-start items-start text-left whitespace-normal p-md rounded-xl hover:bg-surface-container-high hover:border-primary/30 cursor-pointer group"
              onClick={() => onSelectPrompt(ex.prompt)}
            >
              <span className="material-symbols-outlined icon-md text-on-surface-variant mt-0.5 group-hover:text-primary transition-colors">{ex.icon}</span>
              <div className="min-w-0">
                <p className="font-label-md text-on-surface font-bold">{t(ex.titleKey)}</p>
                <p className="font-body-sm text-on-surface-variant line-clamp-2">{ex.prompt}</p>
              </div>
            </Button>
          ))}
        </div>
        <div className="mt-xl flex items-center justify-center gap-lg text-on-surface-variant opacity-50">
          <span className="flex items-center gap-xs text-label-sm"><kbd className="px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono text-[11px]">{formatShortcut('K')}</kbd> {t('welcomeState.shortcuts.commands')}</span>
          <span className="flex items-center gap-xs text-label-sm"><kbd className="px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono text-[11px]">?</kbd> {t('welcomeState.shortcuts.shortcuts')}</span>
          <span className="flex items-center gap-xs text-label-sm"><kbd className="px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono text-[11px]">Alt+Up</kbd> {t('welcomeState.shortcuts.history')}</span>
        </div>
      </div>
    </div>
  )
}
