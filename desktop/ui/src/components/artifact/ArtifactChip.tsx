import { useEffect, useRef } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import type { DetectedArtifact } from './detectArtifact'
import { artifactIcon } from './detectArtifact'
import { artifactDisplayTitle, artifactKindLabel } from './labels'
import { useArtifact } from './ArtifactContext'

interface ArtifactChipProps {
  artifact: DetectedArtifact
}

export function ArtifactChip({ artifact }: ArtifactChipProps) {
  const intl = useIntl()
  const t = useT()
  const { open, autoOpen } = useArtifact()
  const firedRef = useRef(false)

  useEffect(() => {
    if (autoOpen && !firedRef.current) {
      firedRef.current = true
      open(artifact)
    }
  }, [autoOpen, artifact, open])

  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      onClick={() => open(artifact)}
      className={cn('gap-xs border-primary/30 bg-primary/5 text-primary hover:bg-primary/10 hover:border-primary/50 font-label-sm')}
      aria-label={intl.formatMessage({ id: 'chat.artifact.open.aria' }, { kind: artifactKindLabel(artifact.kind, t), title: artifactDisplayTitle(artifact, t) })}
    >
      <span className="material-symbols-outlined icon-sm shrink-0">{artifactIcon(artifact.kind)}</span>
      <span className="truncate max-w-[260px]">{artifactDisplayTitle(artifact, t)}</span>
      <span className="material-symbols-outlined icon-sm shrink-0 opacity-70">open_in_new</span>
    </Button>
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
