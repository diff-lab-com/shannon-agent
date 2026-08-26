import { Button } from '@/components/ui/button'

interface EmptyStateAction {
  label: string
  onClick: () => void
}

// U7: lightweight starter-prompt chips for guide-card style empties (e.g. the
// sidebar zero-session rail). Optional icon mirrors the WelcomeState cards.
interface EmptyStateSuggestion {
  label: string
  icon?: string
  onClick: () => void
}

interface EmptyStateProps {
  icon: string
  title: string
  description?: string
  action?: EmptyStateAction
  suggestions?: EmptyStateSuggestion[]
}

export default function EmptyState({ icon, title, description, action, suggestions }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center py-xl text-center">
      <span className="material-symbols-outlined icon-2xl text-outline-variant mb-md">{icon}</span>
      <h3 className="font-body-lg font-bold text-on-surface mb-xs">{title}</h3>
      {description && (
        <p className="font-body-md text-on-surface-variant max-w-[320px]">{description}</p>
      )}
      {suggestions && suggestions.length > 0 && (
        <div className="mt-lg w-full max-w-[280px] flex flex-col gap-xs">
          {suggestions.map(s => (
            <Button
              key={s.label}
              variant="outline"
              className="justify-start gap-xs px-md py-xs rounded-lg font-label-md text-label-md cursor-pointer"
              onClick={s.onClick}
            >
              {s.icon && <span className="material-symbols-outlined icon-sm" aria-hidden="true">{s.icon}</span>}
              <span className="truncate">{s.label}</span>
            </Button>
          ))}
        </div>
      )}
      {action && (
        <Button className="mt-lg bg-primary text-on-primary px-lg py-sm rounded-xl font-label-md cursor-pointer" onClick={action.onClick}>
          {action.label}
        </Button>
      )}
    </div>
  )
}
