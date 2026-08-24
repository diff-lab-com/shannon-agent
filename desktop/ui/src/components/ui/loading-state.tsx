import { cn } from '@/lib/utils'

interface LoadingStateProps {
  label?: string
  size?: 'sm' | 'md' | 'lg'
}

export default function LoadingState({ label, size = 'md' }: LoadingStateProps) {
  const iconClass = size === 'lg' ? 'text-[48px]' : size === 'sm' ? 'text-[20px]' : 'text-[32px]'
  const pyClass = size === 'lg' ? 'py-3xl' : size === 'sm' ? 'py-sm' : 'py-xl'
  return (
    <div
      role="status"
      aria-live="polite"
      className={cn('flex flex-col items-center justify-center text-center', pyClass)}
    >
      <span className={cn('material-symbols-outlined text-on-surface-variant animate-spin', iconClass)}>
        progress_activity
      </span>
      {label && (
        <p className="font-body-sm text-on-surface-variant mt-sm">{label}</p>
      )}
    </div>
  )
}
