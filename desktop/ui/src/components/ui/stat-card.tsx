// Shared stat tile — icon + value + label. Single implementation for the
// Memory panel and the OPC analytics dashboard (previously two near-identical
// local components with diverging typography).
interface StatCardProps {
  label: string
  value: string | number
  icon: string
}

export default function StatCard({ label, value, icon }: StatCardProps) {
  return (
    <div className="flex items-center gap-sm px-md py-md rounded-xl bg-surface-container-low border border-outline-variant/30">
      <span className="material-symbols-outlined text-primary text-[24px]">{icon}</span>
      <div className="min-w-0">
        <div className="font-headline-md text-[20px] font-bold text-on-surface leading-none">{value}</div>
        <div className="font-label-xs text-on-surface-variant mt-[2px]">{label}</div>
      </div>
    </div>
  )
}
