interface LoadingStateProps {
  label?: string
  size?: 'sm' | 'md' | 'lg'
}

export default function LoadingState({ label, size = 'md' }: LoadingStateProps) {
  const icon = size === 'lg' ? 'text-[48px]' : size === 'sm' ? 'text-[20px]' : 'text-[32px]'
  const py = size === 'lg' ? 'py-3xl' : size === 'sm' ? 'py-sm' : 'py-xl'
  return (
    <div
      role="status"
      aria-live="polite"
      className={`flex flex-col items-center justify-center ${py} text-center`}
    >
      <span className={`material-symbols-outlined ${icon} text-on-surface-variant animate-spin`}>progress_activity</span>
      {label && (
        <p className="font-body-sm text-on-surface-variant mt-sm">{label}</p>
      )}
    </div>
  )
}
