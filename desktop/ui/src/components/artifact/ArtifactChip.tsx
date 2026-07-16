import { useEffect, useRef } from 'react'
import { useIntl } from 'react-intl'
import type { DetectedArtifact } from './detectArtifact'
import { artifactIcon, artifactKindLabel } from './detectArtifact'
import { useArtifact } from './ArtifactContext'

interface ArtifactChipProps {
  artifact: DetectedArtifact
}

export function ArtifactChip({ artifact }: ArtifactChipProps) {
  const intl = useIntl()
  const { open, autoOpen } = useArtifact()
  const firedRef = useRef(false)

  useEffect(() => {
    if (autoOpen && !firedRef.current) {
      firedRef.current = true
      open(artifact)
    }
  }, [autoOpen, artifact, open])

  return (
    <button
      type="button"
      onClick={() => open(artifact)}
      className="inline-flex items-center gap-xs px-sm py-xs rounded-md border border-primary/30 bg-primary/5 text-primary hover:bg-primary/10 hover:border-primary/50 transition-colors font-label-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
      aria-label={intl.formatMessage({ id: 'chat.artifact.open.aria' }, { kind: artifactKindLabel(artifact.kind), title: artifact.title })}
    >
      <span className="material-symbols-outlined icon-sm shrink-0">{artifactIcon(artifact.kind)}</span>
      <span className="truncate max-w-[260px]">{artifact.title}</span>
      <span className="material-symbols-outlined icon-sm shrink-0 opacity-70">open_in_new</span>
    </button>
  )
}

export function ArtifactChipList({ artifacts }: { artifacts: DetectedArtifact[] }) {
  if (artifacts.length === 0) return null
  return (
    <div className="flex flex-wrap gap-xs mt-xs">
      {artifacts.map((art, i) => (
        <ArtifactChip key={`${art.kind}-${i}`} artifact={art} />
      ))}
    </div>
  )
}
