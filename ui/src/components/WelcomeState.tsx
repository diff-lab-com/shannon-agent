interface WelcomeStateProps {
  onSelectPrompt: (prompt: string) => void
}

const EXAMPLES = [
  { icon: 'code', title: 'Write code', prompt: 'Help me build a REST API endpoint in Rust' },
  { icon: 'bug_report', title: 'Debug an issue', prompt: 'Debug why my React component is re-rendering infinitely' },
  { icon: 'school', title: 'Explain a concept', prompt: 'Explain how async/await works in Rust' },
  { icon: 'auto_fix_high', title: 'Refactor', prompt: 'Refactor this function to be more idiomatic' },
]

export default function WelcomeState({ onSelectPrompt }: WelcomeStateProps) {
  return (
    <div className="flex items-center justify-center h-full">
      <div className="text-center max-w-[520px] mx-auto px-lg">
        <div className="w-16 h-16 rounded-full bg-primary-container/30 flex items-center justify-center mx-auto mb-lg">
          <span className="material-symbols-outlined text-[32px] text-primary">auto_awesome</span>
        </div>
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-sm">What can I help with?</h2>
        <p className="font-body-md text-on-surface-variant mb-xl">Ask me to write, debug, explain, or refactor code.</p>
        <div className="grid grid-cols-2 gap-sm">
          {EXAMPLES.map(ex => (
            <button
              key={ex.icon}
              className="flex items-start gap-sm p-md rounded-xl border border-outline-variant/30 bg-surface-container-low hover:bg-surface-container-high hover:border-primary/30 focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none transition-all text-left cursor-pointer group"
              onClick={() => onSelectPrompt(ex.prompt)}
            >
              <span className="material-symbols-outlined text-[20px] text-on-surface-variant mt-0.5 group-hover:text-primary transition-colors">{ex.icon}</span>
              <div>
                <p className="font-label-md text-on-surface font-bold">{ex.title}</p>
                <p className="font-body-sm text-on-surface-variant line-clamp-2">{ex.prompt}</p>
              </div>
            </button>
          ))}
        </div>
        <div className="mt-xl flex items-center justify-center gap-lg text-on-surface-variant opacity-50">
          <span className="flex items-center gap-xs text-label-sm"><kbd className="px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono text-[11px]">Cmd+K</kbd> Commands</span>
          <span className="flex items-center gap-xs text-label-sm"><kbd className="px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono text-[11px]">?</kbd> Shortcuts</span>
          <span className="flex items-center gap-xs text-label-sm"><kbd className="px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono text-[11px]">Alt+Up</kbd> History</span>
        </div>
      </div>
    </div>
  )
}
