import { type ReactNode, useState, memo } from 'react'

// Minimal AI-Elements–style primitives, vendored locally.
// Inspired by Vercel AI Elements (MIT). No runtime deps beyond React.

interface MessageProps {
  from?: 'user' | 'assistant' | 'system'
  className?: string
  children: ReactNode
}

export const Message = memo(function Message({ from, className = '', children }: MessageProps) {
  return (
    <div data-message-from={from} className={className}>
      {children}
    </div>
  )
})

interface MessageAvatarProps {
  from: 'user' | 'assistant' | 'system'
  icon?: string
  className?: string
}

export function MessageAvatar({ from, icon = 'smart_toy', className = '' }: MessageAvatarProps) {
  const bg = from === 'user' ? 'bg-primary' : 'bg-primary-container'
  const fg = from === 'user' ? 'text-on-primary' : 'text-on-primary-container'
  return (
    <div className={`h-10 w-10 rounded-full ${bg} ${fg} flex items-center justify-center shrink-0 shadow-md ${className}`}>
      <span className="material-symbols-outlined" aria-hidden="true">{icon}</span>
    </div>
  )
}

interface MessageContentProps {
  className?: string
  children: ReactNode
}

export const MessageContent = memo(function MessageContent({ className = '', children }: MessageContentProps) {
  return <div className={className}>{children}</div>
})

interface ResponseStreamProps {
  isStreaming?: boolean
  className?: string
  children: ReactNode
}

export function ResponseStream({ isStreaming, className = '', children }: ResponseStreamProps) {
  return (
    <div className={className}>
      {children}
      {isStreaming && (
        <span className="inline-block w-[6px] h-[1em] align-text-bottom ml-[2px] bg-primary animate-pulse rounded-sm" aria-hidden="true" />
      )}
    </div>
  )
}

interface ReasoningProps {
  defaultOpen?: boolean
  className?: string
  header?: ReactNode
  children: ReactNode
}

export function Reasoning({ defaultOpen = false, className = '', header, children }: ReasoningProps) {
  const [open, setOpen] = useState(defaultOpen)
  return (
    <div className={`rounded-lg border border-outline-variant/20 ${className}`}>
      <button
        type="button"
        onClick={() => setOpen(o => !o)}
        aria-expanded={open}
        className="flex w-full items-center gap-xs px-sm py-xs text-on-surface-variant hover:bg-surface-container-low/60 transition-colors"
      >
        <span className="material-symbols-outlined text-[16px]">{open ? 'expand_less' : 'expand_more'}</span>
        <span className="material-symbols-outlined text-[16px]">psychology</span>
        <span className="font-label-md flex-1 text-left">{header ?? 'Reasoning'}</span>
      </button>
      {open && <div className="px-md pb-md text-body-sm text-on-surface-variant prose prose-sm max-w-none">{children}</div>}
    </div>
  )
}

interface ToolProps {
  name?: string
  status?: 'running' | 'completed' | 'error'
  className?: string
  children: ReactNode
}

export function Tool({ name, status, className = '', children }: ToolProps) {
  return (
    <div data-tool-name={name} data-tool-status={status} className={`rounded-xl border border-outline-variant/10 bg-surface-container-low ${className}`}>
      {children}
    </div>
  )
}

interface ToolHeaderProps {
  onClick?: () => void
  className?: string
  children: ReactNode
}

export function ToolHeader({ onClick, className = '', children }: ToolHeaderProps) {
  return (
    <div
      className={`flex items-center gap-sm ${onClick ? 'cursor-pointer' : ''} ${className}`}
      onClick={onClick}
      role={onClick ? 'button' : undefined}
      tabIndex={onClick ? 0 : undefined}
    >
      {children}
    </div>
  )
}

interface ToolContentProps {
  className?: string
  children: ReactNode
}

export function ToolContent({ className = '', children }: ToolContentProps) {
  return <div className={`mt-sm space-y-xs ${className}`}>{children}</div>
}

interface ActionToolbarProps {
  className?: string
  children: ReactNode
}

export function ActionToolbar({ className = '', children }: ActionToolbarProps) {
  return <div className={`flex gap-sm ${className}`}>{children}</div>
}
