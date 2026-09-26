import { useEffect } from 'react'
import { useIntl } from 'react-intl'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import type { DetectedArtifact } from './detectArtifact'
import { artifactIcon } from './detectArtifact'
import { artifactDisplayTitle, artifactKindLabel } from './labels'
import { useArtifact } from './ArtifactContext'

interface ArtifactChipProps {
  artifact: DetectedArtifact
}

/**
 * Batch C1 (2026-09-20 delta analysis): the artifact chip grew into the
 * ZCode 产物卡 form — kind icon tile + display title + localized kind badge
 * (「文档」) + an explicit 打开 affordance, one card per detected artifact.
 * The whole card is one button; the trailing 打开 pill is decorative.
 *
 * B3 §P1-11: auto-open bookkeeping moved into the provider
 * (`autoOpenOnce`) — this component remounts every time the virtualized
 * message list scrolls it out of view, so a per-mount ref re-fired the
 * open on each scroll cycle (tab storm with autoOpen enabled).
 */
export function ArtifactChip({ artifact }: ArtifactChipProps) {
  const intl = useIntl()
  const t = useT()
  const { open, autoOpen, autoOpenOnce } = useArtifact()

  useEffect(() => {
    if (autoOpen) autoOpenOnce(artifact)
  }, [autoOpen, artifact, autoOpenOnce])

  return (
    <button
      type="button"
      onClick={() => open(artifact)}
      aria-label={intl.formatMessage({ id: 'chat.artifact.open.aria' }, { kind: artifactKindLabel(artifact.kind, t), title: artifactDisplayTitle(artifact, t) })}
      data-testid="artifact-card"
      className={cn(
        'group/card flex items-center gap-sm max-w-md w-full px-sm py-xs rounded-xl text-left',
        'border border-primary/25 bg-primary/5 hover:bg-primary/10 hover:border-primary/40',
        'transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30',
      )}
    >
      <span className="w-8 h-8 rounded-lg bg-primary/15 flex items-center justify-center shrink-0" aria-hidden="true">
        <span className="material-symbols-outlined text-[18px] text-primary">{artifactIcon(artifact.kind)}</span>
      </span>
      <span className="flex-1 min-w-0">
        <span className="block font-label-md text-on-surface truncate">{artifactDisplayTitle(artifact, t)}</span>
        <span className="block font-label-xs text-on-surface-variant">{artifactKindLabel(artifact.kind, t)}</span>
      </span>
      <span
        className="flex items-center gap-[2px] shrink-0 px-xs py-[2px] rounded-md bg-primary/10 text-primary font-label-xs group-hover/card:bg-primary/20"
        aria-hidden="true"
      >
        {t('chat.artifact.open')}
        <span className="material-symbols-outlined text-[13px]">open_in_new</span>
      </span>
    </button>
  )
}

export function ArtifactChipList({ artifacts }: { artifacts: DetectedArtifact[] }) {
  if (artifacts.length === 0) return null
  return (
    <div className="flex flex-col gap-xs mt-xs">
      {artifacts.map((art, i) => (
        <ArtifactChip key={`${art.kind}-${i}`} artifact={art} />
      ))}
    </div>
  )
}
