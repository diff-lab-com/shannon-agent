// Batch D (2026-09-20 delta analysis §批D): the right-dock reader.
//  - D1: heading parsing/slug rules shared by renderer ids and the TOC rail;
//    the TOC rail renders for multi-section docs
//  - D3: artifact documents render code fences through the shared CodeBlock
//    (copy button + language chrome)
//  - D5: the plan panel's checkbox writeback flips the right checklist item
//    and persists through saveTextFile with the engine's header layout

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { DocumentRenderer } from '@/components/artifact/DocumentRenderer'
import { DocumentToc } from '@/components/artifact/DocumentToc'
import { parseDocHeadings, slugifyHeading } from '@/components/artifact/docToc'

const PLAN_FIXTURE = vi.hoisted(() => ({
  id: 'plan-1',
  title: 'P0 基线',
  status: 'pending',
  created_at: '2026-09-20T00:00:00Z',
  content: '# P0 基线\n\n- [ ] 第一步\n- [ ] 第二步\n',
}))

vi.mock('@/lib/tauri-api', () => ({
  getSessionPlan: vi.fn(async () => PLAN_FIXTURE),
  saveTextFile: vi.fn(async () => {}),
  save: vi.fn(async () => null),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn(async () => null),
}))

const DOC = `# 调研报告

正文一段。

## 项目概述

内容 A。

## 项目概述

重复标题去重。

### 实现细节

代码块里的 # 不是标题：

\`\`\`bash
# this is a comment
\`\`\`

## 结论
`

describe('parseDocHeadings / slugifyHeading (D1)', () => {
  it('parses h1-h3, skips fenced lines, dedups repeated titles', () => {
    const hs = parseDocHeadings(DOC)
    expect(hs.map(h => h.text)).toEqual(['调研报告', '项目概述', '项目概述', '实现细节', '结论'])
    expect(hs.map(h => h.id)).toEqual(['调研报告', '项目概述', '项目概述-1', '实现细节', '结论'])
    expect(hs[3].level).toBe(3)
  })

  it('slugifies CJK and latin consistently', () => {
    expect(slugifyHeading('Web Video Clone 方案!')).toBe('web-video-clone-方案')
  })

  it('renderer heading ids match the parsed TOC ids', () => {
    const { container } = render(
      <I18nProvider>
        <DocumentRenderer source={DOC} />
      </I18nProvider>,
    )
    for (const h of parseDocHeadings(DOC)) {
      expect(container.querySelector(`h1#${CSS.escape(h.id)}, h2#${CSS.escape(h.id)}, h3#${CSS.escape(h.id)}`)).toBeTruthy()
    }
  })
})

describe('DocumentToc rail (D1)', () => {
  it('renders nothing below the heading threshold', () => {
    const { container } = render(
      <I18nProvider>
        <div id="dock-tabpanel" />
        <DocumentToc source="# Only one" />
      </I18nProvider>,
    )
    expect(container.querySelector('[data-testid="document-toc"]')).toBeNull()
  })

  it('renders the numbered rail for multi-section documents', () => {
    render(
      <I18nProvider>
        <div id="dock-tabpanel" />
        <DocumentToc source={DOC} />
      </I18nProvider>,
    )
    const toc = screen.getByTestId('document-toc')
    expect(toc.querySelectorAll('li')).toHaveLength(5)
    expect(toc.textContent).toContain('结论')
  })
})

describe('artifact code blocks use the shared CodeBlock (D3)', () => {
  it('renders a copy affordance for fenced code', () => {
    const md = '```bash\necho hi\n```'
    const { container } = render(
      <I18nProvider>
        <DocumentRenderer source={md} />
      </I18nProvider>,
    )
    // the shared CodeBlock chrome renders a copy button
    expect(container.querySelector('button[aria-label]')).toBeTruthy()
    expect(container.textContent!.toLowerCase()).toContain('copy')
  })
})

describe('plan checkbox writeback (D5)', () => {
  beforeEach(() => {
    // mockClear only — the factory implementations stay wired.
    vi.clearAllMocks()
  })

  async function renderPlanPanel(workingDir = '/repo') {
    const { default: PlanPanel } = await import('@/pages/chat/PlanPanel')
    const result = render(
      <I18nProvider>
        <PlanPanel workingDir={workingDir} planModeActive={false} />
      </I18nProvider>,
    )
    // the plan title renders twice (header + content h1) — wait for either
    await screen.findAllByText('P0 基线')
    return result
  }

  it('flips the clicked checklist item and writes the plan file back', async () => {
    const { container } = await renderPlanPanel()
    const boxes = container.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')
    expect(boxes.length).toBe(2)
    fireEvent.click(boxes[1])
    const api = vi.mocked(await import('@/lib/tauri-api'))
    await waitFor(() => expect(api.saveTextFile).toHaveBeenCalledTimes(1))
    const [path, content] = api.saveTextFile.mock.calls[0]
    expect(path).toBe('/repo/.shannon/plans/plan-1.md')
    expect(content).toContain('- [ ] 第一步')
    expect(content).toContain('- [x] 第二步')
    // header layout matches the engine's PlanManager::save_plan_to_file
    expect(content.startsWith('# Plan: P0 基线\nCreated: 2026-09-20T00:00:00Z\nStatus: pending\n\n')).toBe(true)
  })

  it('ticking a step never forges engine approval status', async () => {
    const { container } = await renderPlanPanel()
    const boxes = container.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')
    fireEvent.click(boxes[0])
    const api = vi.mocked(await import('@/lib/tauri-api'))
    await waitFor(() => expect(api.saveTextFile).toHaveBeenCalledTimes(1))
    const content = api.saveTextFile.mock.calls[0][1]
    expect(content).toContain('Status: pending')
  })
})
