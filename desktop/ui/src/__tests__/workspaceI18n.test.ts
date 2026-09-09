// P1-5 C-2 — i18n completeness: every workspace.* key must exist in BOTH
// locales (en is the fallback; zh-CN must not drift).

import { describe, expect, it } from 'vitest'
import en from '@/i18n/locales/en.json'
import zh from '@/i18n/locales/zh-CN.json'

describe('workspace i18n keys', () => {
  const workspaceKeys = Object.keys(en).filter(k => k.startsWith('workspace.'))

  it('defines workspace keys in en.json', () => {
    expect(workspaceKeys.length).toBeGreaterThan(20)
  })

  it('mirrors every workspace key in zh-CN.json', () => {
    const missing = workspaceKeys.filter(k => !(k in zh))
    expect(missing).toEqual([])
  })

  it('leaves workspace values non-empty in both locales', () => {
    for (const key of workspaceKeys) {
      expect(String(en[key as keyof typeof en]).length).toBeGreaterThan(0)
      expect(String(zh[key as keyof typeof zh]).length).toBeGreaterThan(0)
    }
  })
})
