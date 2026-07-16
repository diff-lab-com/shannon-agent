import { useIntl } from 'react-intl'
import { Button } from './button'

interface PaginationProps {
  page: number
  totalPages: number
  onPageChange: (page: number) => void
}

export function Pagination({ page, totalPages, onPageChange }: PaginationProps) {
  const intl = useIntl()
  if (totalPages <= 1) return null

  const pages: (number | '…')[] = []
  if (totalPages <= 7) {
    for (let i = 1; i <= totalPages; i++) pages.push(i)
  } else {
    pages.push(1)
    if (page > 3) pages.push('…')
    for (let i = Math.max(2, page - 1); i <= Math.min(totalPages - 1, page + 1); i++) {
      pages.push(i)
    }
    if (page < totalPages - 2) pages.push('…')
    pages.push(totalPages)
  }

  return (
    <div className="flex items-center justify-center gap-xs py-md">
      <Button
        variant="ghost"
        disabled={page <= 1}
        onClick={() => onPageChange(page - 1)}
        className="px-sm py-xs rounded-lg text-on-surface-variant hover:text-primary disabled:opacity-30"
        aria-label={intl.formatMessage({ id: 'ui.pagination.previous' })}
      >
        <span className="material-symbols-outlined text-[18px]">chevron_left</span>
      </Button>
      {pages.map((p, i) =>
        p === '…' ? (
          <span key={`ellipsis-${i}`} className="px-xs text-on-surface-variant text-label-sm">…</span>
        ) : (
          <Button
            key={p}
            variant="ghost"
            onClick={() => onPageChange(p)}
            className={`min-w-[32px] px-sm py-xs rounded-lg text-label-sm font-label-md ${
              p === page ? 'bg-primary/10 text-primary font-bold' : 'text-on-surface-variant hover:text-primary'
            }`}
          >
            {p}
          </Button>
        )
      )}
      <Button
        variant="ghost"
        disabled={page >= totalPages}
        onClick={() => onPageChange(page + 1)}
        className="px-sm py-xs rounded-lg text-on-surface-variant hover:text-primary disabled:opacity-30"
        aria-label={intl.formatMessage({ id: 'ui.pagination.next' })}
      >
        <span className="material-symbols-outlined text-[18px]">chevron_right</span>
      </Button>
    </div>
  )
}
