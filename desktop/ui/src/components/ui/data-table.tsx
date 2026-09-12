import { useState } from 'react'
import {
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  useReactTable,
  type ColumnDef,
  type SortingState,
} from '@tanstack/react-table'
import { cn } from '@/lib/utils'

interface DataTableProps<TData> {
  columns: ColumnDef<TData, unknown>[]
  data: TData[]
  /** Called with the row id on click; omit for non-interactive tables. */
  onRowClick?: (row: TData) => void
  emptyMessage?: string
  className?: string
}

/**
 * Token-styled sortable data table (shadcn copy-in pattern on TanStack
 * Table v9). Sorting is opt-in per column via `enableSorting`; a click on
 * the header toggles asc → desc. Glass/elevation styling comes from the
 * theme tokens, not from this primitive.
 */
export function DataTable<TData>({ columns, data, onRowClick, emptyMessage, className }: DataTableProps<TData>) {
  const [sorting, setSorting] = useState<SortingState>([])
  const table = useReactTable({
    data,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  })

  return (
    <div className={cn('rounded-xl border border-outline-variant/30 overflow-hidden', className)}>
      <table className="w-full text-label-md">
        <thead>
          {table.getHeaderGroups().map(hg => (
            <tr key={hg.id} className="bg-surface-container-low border-b border-outline-variant/30">
              {hg.headers.map(header => (
                <th
                  key={header.id}
                  {...{ onClick: header.column.getCanSort() ? header.column.getToggleSortingHandler() : undefined }}
                  className={cn(
                    'px-md py-sm text-left font-label-sm font-bold text-on-surface-variant uppercase tracking-wide',
                    header.column.getCanSort() && 'cursor-pointer select-none hover:text-on-surface',
                  )}
                  aria-sort={header.column.getIsSorted() === 'asc' ? 'ascending' : header.column.getIsSorted() === 'desc' ? 'descending' : 'none'}
                >
                  {header.isPlaceholder ? null : flexRender(header.column.columnDef.header, header.getContext())}
                  {{ asc: ' ↑', desc: ' ↓' }[header.column.getIsSorted() as string] ?? ''}
                </th>
              ))}
            </tr>
          ))}
        </thead>
        <tbody>
          {table.getRowModel().rows.length === 0 ? (
            <tr>
              <td colSpan={columns.length} className="px-md py-lg text-center text-on-surface-variant">
                {emptyMessage}
              </td>
            </tr>
          ) : (
            table.getRowModel().rows.map((row, i) => (
              <tr
                key={row.id ?? i}
                onClick={onRowClick ? () => onRowClick(row.original) : undefined}
                className={cn(
                  'border-b border-outline-variant/20 last:border-0 bg-surface-container-lowest hover:bg-surface-container-low transition-colors',
                  onRowClick && 'cursor-pointer',
                )}
              >
                {row.getVisibleCells().map(cell => (
                  <td key={cell.id} className="px-md py-sm text-on-surface">
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </td>
                ))}
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  )
}
