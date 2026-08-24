// Memory panel — single stat tile (label + count + icon). Extracted from
// MemoryPanel.tsx (T3.1).
interface StatCardProps {
  label: string
  value: number
  icon: string
}

export function StatCard({ label, value, icon }: StatCardProps) {
  return (
    <div className="flex items-center gap-sm px-md py-md rounded-xl bg-surface-container-low border border-outline-variant/30">
      <span className="material-symbols-outlined text-primary text-[24px]">{icon}</span>
      <div>
        <div className="text-label-lg font-bold text-on-surface leading-none">{value}</div>
        <div className="text-label-xs text-on-surface-variant mt-[2px]">{label}</div>
      </div>
    </div>
  )
}
