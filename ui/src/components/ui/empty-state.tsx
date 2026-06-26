import { Button } from '@/components/ui/button'

interface EmptyStateAction {
  label: string
  onClick: () => void
}

interface EmptyStateProps {
  icon: string
  title: string
  description?: string
  action?: EmptyStateAction
}

export default function EmptyState({ icon, title, description, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center py-xl text-center">
      <span className="material-symbols-outlined icon-2xl text-outline-variant mb-md">{icon}</span>
      <h3 className="font-body-lg font-bold text-on-surface mb-xs">{title}</h3>
      {description && (
        <p className="font-body-md text-on-surface-variant max-w-[320px]">{description}</p>
      )}
      {action && (
        <Button className="mt-lg bg-primary text-on-primary px-lg py-sm rounded-xl font-label-md cursor-pointer" onClick={action.onClick}>
          {action.label}
        </Button>
      )}
    </div>
  )
}
