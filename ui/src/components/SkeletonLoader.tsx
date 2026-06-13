export function Skeleton({ className = '' }: { className?: string }) {
  return <div className={`animate-pulse bg-outline-variant/20 rounded-md ${className}`} />
}

export function CardSkeleton() {
  return (
    <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
      <div className="flex items-center gap-2 mb-6">
        <Skeleton className="w-5 h-5 rounded-full" />
        <Skeleton className="h-5 w-32" />
      </div>
      <Skeleton className="h-4 w-full mb-3" />
      <Skeleton className="h-4 w-3/4 mb-3" />
      <Skeleton className="h-4 w-1/2" />
    </div>
  )
}

export function ListSkeleton({ count = 3 }: { count?: number }) {
  return (
    <div className="space-y-md">
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="flex items-center gap-md p-md">
          <Skeleton className="w-10 h-10 rounded-xl shrink-0" />
          <div className="flex-1 space-y-sm">
            <Skeleton className="h-4 w-2/3" />
            <Skeleton className="h-3 w-1/2" />
          </div>
        </div>
      ))}
    </div>
  )
}

export function MetricsSkeleton() {
  return (
    <div className="grid grid-cols-2 gap-sm">
      {Array.from({ length: 4 }).map((_, i) => (
        <div key={i} className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/20">
          <Skeleton className="h-3 w-16 mb-2" />
          <Skeleton className="h-6 w-20" />
        </div>
      ))}
    </div>
  )
}
