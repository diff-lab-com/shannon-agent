// Batch A (2026-09-20 delta analysis): localized artifact labels + composer
// placeholder interpolation. Guards:
//  - artifact.kind.* / artifact.fallback.* exist in en + zh-CN (the two
//    maintained locales; others fall back to en at runtime)
//  - chat.input.placeholder.project carries the {dir} interpolation instead
//    of the old broken "正在 —" concatenation
//  - the removed dead time-bucket keys stay removed

import { describe, expect, it } from 'vitest'
import en from '@/i18n/locales/en.json'
import zh from '@/i18n/locales/zh-CN.json'
import { artifactDisplayTitle, artifactKindLabel } from '@/components/artifact/labels'
import type { DetectedArtifact } from '@/components/artifact/detectArtifact'

const KINDS = ['html', 'svg', 'mermaid', 'document'] as const
const DEAD_KEYS = [
  'sidebar.sessions.group.today',
  'sidebar.sessions.group.yesterday',
  'sidebar.sessions.group.thisWeek',
  'sidebar.sessions.group.earlier',
  'sidebar.sessions.grouping.time',
  'sidebar.sessions.grouping.session.desc',
]

const t = (id: string) => {
  const table = { 'artifact.kind.html': 'HTML', 'artifact.kind.svg': 'SVG', 'artifact.kind.mermaid': '图示', 'artifact.kind.document': '文档', 'artifact.fallback.html': 'HTML 文档', 'artifact.fallback.svg': 'SVG 图形', 'artifact.fallback.mermaid': 'Mermaid 图', 'artifact.fallback.document': '文档' } as Record<string, string>
  return table[id]
}

describe('artifact label i18n', () => {
  it.each(KINDS)('defines artifact.kind.%s and artifact.fallback.%s in both locales', kind => {
    for (const table of [en, zh] as Record<string, string>[]) {
      expect(table[`artifact.kind.${kind}`], kind).toBeTruthy()
      expect(table[`artifact.fallback.${kind}`], kind).toBeTruthy()
    }
  })

  it('placeholder.project uses {dir} interpolation in both locales', () => {
    for (const table of [en, zh] as Record<string, string>[]) {
      expect(table['chat.input.placeholder.project']).toContain('{dir}')
    }
  })

  it('removed dead time-bucket keys stay removed', () => {
    for (const key of DEAD_KEYS) {
      expect(key in en, key).toBe(false)
      expect(key in zh, key).toBe(false)
    }
  })
})

describe('artifactDisplayTitle', () => {
  it('uses the detected title when present', () => {
    const a = { kind: 'document', title: '调研报告', source: '', confidence: 'medium' } as DetectedArtifact
    expect(artifactDisplayTitle(a, t)).toBe('调研报告')
  })

  it('falls back to a localized per-kind title when detection found none', () => {
    const a = { kind: 'svg', title: '', source: '', confidence: 'high' } as DetectedArtifact
    expect(artifactDisplayTitle(a, t)).toBe('SVG 图形')
  })

  it('artifactKindLabel resolves through the translator', () => {
    expect(artifactKindLabel('document', t)).toBe('文档')
  })
})
