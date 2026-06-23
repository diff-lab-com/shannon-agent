import { useMemo } from 'react'
import { useIntl } from 'react-intl'
import { computeHunks, type HunkDecision } from '@/lib/diff-merge'
import type { FileDiff } from '@/types'

export type FileFilter = 'all' | 'unreviewed' | 'partial' | 'accepted'

interface FileDiffListProps {
  files: string[]
  diffs: Map<string, FileDiff>
  decisions: Map<string, Map<string, HunkDecision>>
  currentPath: string | null
  filter: FileFilter
  onSelectPath: (path: string) => void
  onFilterChange: (filter: FileFilter) => void
}

interface FileStatus {
  total: number
  accepted: number
  rejected: number
  pending: number
  label: string
  badgeStyle: string
}

function fileStatus(
  path: string,
  diff: FileDiff | undefined,
  decisions: Map<string, Map<string, HunkDecision>>,
  intl: ReturnType<typeof useIntl>,
): FileStatus {
  if (!diff) {
    return {
      total: 0,
      accepted: 0,
      rejected: 0,
      pending: 0,
      label: intl.formatMessage({ id: 'diff.multi.file.status.unreviewed' }),
      badgeStyle: 'bg-surface-container-high text-on-surface-variant',
    }
  }
  const hunks = computeHunks(diff.old_content, diff.new_content)
  const fileDecisions = decisions.get(path) ?? new Map<string, HunkDecision>()
  let accepted = 0
  let rejected = 0
  let pending = 0
  for (const h of hunks) {
    const d = fileDecisions.get(h.id) ?? 'pending'
    if (d === 'accept') accepted += 1
    else if (d === 'reject') rejected += 1
    else pending += 1
  }
  const total = hunks.length
  let label: string
  let badgeStyle: string
  if (accepted === total && total > 0) {
    label = intl.formatMessage({ id: 'diff.multi.file.status.allAccepted' })
    badgeStyle = 'bg-tertiary-container/60 text-tertiary'
  } else if (rejected === total && total > 0) {
    label = intl.formatMessage({ id: 'diff.multi.file.status.allRejected' })
    badgeStyle = 'bg-error-container/60 text-error'
  } else if (total === 0) {
    label = intl.formatMessage({ id: 'diff.multi.file.status.unreviewed' })
    badgeStyle = 'bg-surface-container-high text-on-surface-variant'
  } else {
    label = intl.formatMessage({ id: 'diff.multi.file.status.partial' }, { accepted, total })
    badgeStyle = 'bg-surface-container-high text-on-surface-variant'
  }
  return { total, accepted, rejected, pending, label, badgeStyle }
}

const FILTERS: FileFilter[] = ['all', 'unreviewed', 'partial', 'accepted']

export default function FileDiffList({
  files,
  diffs,
  decisions,
  currentPath,
  filter,
  onSelectPath,
  onFilterChange,
}: FileDiffListProps) {
  const intl = useIntl()

  const visibleFiles = useMemo(() => {
    return files.filter(path => {
      if (filter === 'all') return true
      const status = fileStatus(path, diffs.get(path), decisions, intl)
      if (filter === 'unreviewed') return status.accepted === 0 && status.rejected === 0
      if (filter === 'partial') {
        const decided = status.accepted + status.rejected
        return decided > 0 && decided < status.total
      }
      if (filter === 'accepted') return status.accepted > 0
      return true
    })
  }, [files, filter, diffs, decisions, intl])

  return (
    <aside className="w-64 shrink-0 border-r border-outline-variant/30 bg-surface-container-low flex flex-col">
      <div className="px-md py-sm border-b border-outline-variant/30">
        <div className="flex items-center gap-xs overflow-x-auto">
          {FILTERS.map(f => (
            <button
              key={f}
              type="button"
              onClick={() => onFilterChange(f)}
              className={`px-xs py-[2px] rounded-full font-label-sm whitespace-nowrap cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary ${
                filter === f
                  ? 'bg-primary text-on-primary'
                  : 'bg-surface-container-high text-on-surface-variant hover:bg-surface-container-highest'
              }`}
            >
              {intl.formatMessage({ id: `diff.multi.filter.${f}` })}
            </button>
          ))}
        </div>
      </div>
      <ul className="flex-1 overflow-auto">
        {visibleFiles.length === 0 ? (
          <li className="px-md py-md font-body-sm text-on-surface-variant italic">
            {intl.formatMessage({ id: 'diff.multi.empty' })}
          </li>
        ) : (
          visibleFiles.map(path => {
            const diff = diffs.get(path)
            const status = fileStatus(path, diff, decisions, intl)
            const isActive = path === currentPath
            const adds = diff ? computeAddedCount(diff) : 0
            const dels = diff ? computeRemovedCount(diff) : 0
            return (
              <li key={path}>
                <button
                  type="button"
                  onClick={() => onSelectPath(path)}
                  className={`w-full text-left px-md py-sm border-b border-outline-variant/20 cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary ${
                    isActive ? 'bg-secondary-container/40' : 'hover:bg-surface-container-high'
                  }`}
                  aria-label={intl.formatMessage({ id: 'diff.multi.file.aria' }, { path })}
                  aria-current={isActive ? 'true' : undefined}
                >
                  <div className="flex items-center gap-xs min-w-0">
                    <span className="material-symbols-outlined text-[16px] text-on-surface-variant shrink-0">
                      description
                    </span>
                    <code className="font-label-sm text-on-surface truncate flex-1 min-w-0">{path}</code>
                  </div>
                  <div className="flex items-center gap-xs mt-xs ml-[24px]">
                    {diff && (
                      <span className="font-label-sm text-tertiary shrink-0">+{adds}</span>
                    )}
                    {diff && (
                      <span className="font-label-sm text-error shrink-0">−{dels}</span>
                    )}
                    <span className={`font-label-sm px-xs rounded-full ml-auto ${status.badgeStyle}`}>
                      {status.label}
                    </span>
                  </div>
                </button>
              </li>
            )
          })
        )}
      </ul>
    </aside>
  )
}

function computeAddedCount(diff: FileDiff): number {
  const hunks = computeHunks(diff.old_content, diff.new_content)
  let count = 0
  for (const h of hunks) {
    for (const line of h.lines) {
      if (line.type === 'added') count += 1
    }
  }
  return count
}

function computeRemovedCount(diff: FileDiff): number {
  const hunks = computeHunks(diff.old_content, diff.new_content)
  let count = 0
  for (const h of hunks) {
    for (const line of h.lines) {
      if (line.type === 'removed') count += 1
    }
  }
  return count
}
