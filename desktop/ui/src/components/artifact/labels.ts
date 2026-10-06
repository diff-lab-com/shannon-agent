// Localized display strings for detected artifacts. detectArtifact.ts stays
// a pure detector (no intl imports) — every user-visible string resolves
// here so the active locale decides the wording (batch A3, 2026-09-20 delta
// analysis: hardcoded English labels leaked into non-English UIs).
import type { ArtifactKind, DetectedArtifact } from './detectArtifact'

/** Minimal shape of the `useT()` translator — keeps this file render-free. */
type TFn = (id: string, values?: Record<string, string>) => string

const KIND_LABEL_KEY: Record<ArtifactKind, string> = {
  html: 'artifact.kind.html',
  svg: 'artifact.kind.svg',
  mermaid: 'artifact.kind.mermaid',
  document: 'artifact.kind.document',
  image: 'artifact.kind.image',
  web: 'artifact.kind.web',
  other: 'artifact.kind.other',
}

const FALLBACK_TITLE_KEY: Record<ArtifactKind, string> = {
  html: 'artifact.fallback.html',
  svg: 'artifact.fallback.svg',
  mermaid: 'artifact.fallback.mermaid',
  document: 'artifact.fallback.document',
  image: 'artifact.fallback.image',
  web: 'artifact.fallback.web',
  other: 'artifact.fallback.other',
}

export function artifactKindLabel(kind: ArtifactKind, t: TFn): string {
  return t(KIND_LABEL_KEY[kind])
}

/** Detected title (e.g. first heading) or a localized per-kind fallback. */
export function artifactDisplayTitle(artifact: DetectedArtifact, t: TFn): string {
  return artifact.title || t(FALLBACK_TITLE_KEY[artifact.kind])
}
