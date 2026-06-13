// Task Completion card — shows completed/total ratio with a progress bar.
//
// MD3 tokens only. Variant: compact (calendar-mode bottom widget) or full
// (sidebar widget with description and decorative icon).

interface EfficiencyCardProps {
  /** 0-100 completion percentage. */
  percentage: number
  /** Show the description and decorative icon (sidebar layout). */
  variant?: 'compact' | 'full'
}

export default function EfficiencyCard({ percentage, variant = 'full' }: EfficiencyCardProps) {
  if (variant === 'compact') {
    return (
      <div className="bg-primary overflow-hidden rounded-2xl relative p-lg text-on-primary">
        <div className="relative z-10">
          <h4 className="font-label-md text-on-primary/80 uppercase tracking-widest mb-md">Task Completion</h4>
          <div className="text-display-lg text-[40px] mb-xs">{percentage}%</div>
          <div className="mt-lg h-2 bg-surface-container-lowest/20 rounded-full overflow-hidden">
            <div className="h-full bg-surface-container-lowest" style={{ width: `${percentage}%` }} />
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="bg-primary overflow-hidden rounded-2xl relative p-lg text-on-primary">
      <div className="relative z-10">
        <h4 className="font-label-md text-on-primary/80 uppercase tracking-widest mb-md">Task Completion</h4>
        <div className="text-display-lg text-[40px] mb-xs">{percentage}%</div>
        <p className="font-body-sm text-on-primary/70">Share of tasks marked completed in the current view.</p>
        <div className="mt-lg h-2 bg-surface-container-lowest/20 rounded-full overflow-hidden">
          <div className="h-full bg-surface-container-lowest" style={{ width: `${percentage}%` }} />
        </div>
      </div>
      <div className="absolute -right-8 -bottom-8 opacity-20 transform rotate-12 pointer-events-none">
        <span className="material-symbols-outlined text-[120px]" style={{ fontVariationSettings: "'FILL' 1" }}>auto_awesome</span>
      </div>
    </div>
  )
}
