// Shared stat tile — icon + value + label. Single implementation for the
// Memory panel, the OPC analytics dashboard, and the Usage page (previously
// three near-identical local components with diverging typography; UI review
// 2026-09-29 Batch 1 P2 merged them — typography follows this component).
interface StatCardProps {
  label: string
  value: string | number
  icon: string
  /** Short context line pinned to the card's right edge (truncated, full
   *  text on title hover) — e.g. the Usage page's cache hint. */
  hint?: string
}

export default function StatCard({ label, value, icon, hint }: StatCardProps) {
  return (
    // h-full so grid rows with mixed hint/no-hint cards stay equal height.
    <div className="flex items-center gap-sm px-md py-md rounded-xl bg-surface-container-low border border-outline-variant/30 h-full">
      <span className="material-symbols-outlined text-primary icon-lg">{icon}</span>
      <div className="min-w-0">
        <div className="font-headline-md text-headline-sm font-bold text-on-surface leading-none">{value}</div>
        <div className="font-label-xs text-on-surface-variant mt-[2px]">{label}</div>
      </div>
      {hint && (
        <span
          className="ml-auto shrink-0 font-label-xs text-label-xs text-on-surface-variant truncate max-w-[40%] text-right"
          title={hint}
        >
          {hint}
        </span>
      )}
    </div>
  )
}
