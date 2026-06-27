import { Button } from '@/components/ui/button'

interface ErrorStateAction {
  label: string
  onClick: () => void
}

interface ErrorStateProps {
  icon?: string
  title: string
  description?: string
  action?: ErrorStateAction
}

export default function ErrorState({ icon = 'error', title, description, action }: ErrorStateProps) {
  return (
    <div
      role="alert"
      className="flex flex-col items-center justify-center py-xl text-center"
    >
      <span className="material-symbols-outlined icon-2xl text-error mb-md">{icon}</span>
      <h3 className="font-body-lg font-bold text-on-surface mb-xs">{title}</h3>
      {description && (
        <p className="font-body-md text-on-surface-variant max-w-[420px]">{description}</p>
      )}
      {action && (
        <Button
          className="mt-lg bg-primary text-on-primary px-lg py-sm rounded-xl font-label-md cursor-pointer"
          onClick={action.onClick}
        >
          {action.label}
        </Button>
      )}
    </div>
  )
}
