// P-U3 — the removable project filter chip shared by the project-scoped
// pages (/tasks?project=, /triage?project=). Purely presentational: the
// label is resolved by the caller (useProjectDeepLink — registry name or
// path tail) and × simply strips the `project` search param.
import { useIntl } from 'react-intl'

export default function ProjectFilterChip({ label, onRemove }: { label: string; onRemove: () => void }) {
  const intl = useIntl()
  return (
    <div
      data-testid="project-filter-chip"
      aria-label={intl.formatMessage({ id: 'project.filter.chip.aria' }, { name: label })}
      title={intl.formatMessage({ id: 'project.filter.chip.aria' }, { name: label })}
      className="inline-flex items-center gap-xs px-sm py-xs rounded-full bg-primary/10 border border-primary/20 text-primary font-label-sm mb-md max-w-full"
    >
      <span className="material-symbols-outlined text-[14px] shrink-0" aria-hidden="true">folder</span>
      <span className="font-bold truncate min-w-0">{label}</span>
      <button
        type="button"
        data-testid="project-filter-chip-remove"
        aria-label={intl.formatMessage({ id: 'project.filter.remove.aria' })}
        title={intl.formatMessage({ id: 'project.filter.remove.aria' })}
        onClick={onRemove}
        className="ml-[2px] w-4 h-4 -mr-[2px] rounded-full hover:bg-primary/20 cursor-pointer flex items-center justify-center shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
      >
        <span className="material-symbols-outlined text-[13px]" aria-hidden="true">close</span>
      </button>
    </div>
  )
}
