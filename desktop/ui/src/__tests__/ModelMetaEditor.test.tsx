// S2-2 — ModelMetaEditor component tests.
//
// Pins the editor's three user-facing contracts:
//   1. edit → save emits a VALID, complete DeclaredModelInput (display name
//      and non-editor capabilities preserved),
//   2. invalid input is intercepted inline — onSave never fires,
//   3. cancel backs out without calling onSave.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import ModelMetaEditor from '@/components/settings/models-settings/ModelMetaEditor'
import type { DeclaredModelSpec } from '@/types'

const SPEC: DeclaredModelSpec = {
  id: 'proxy-model-x',
  display_name: 'Proxy X',
  context_window: 198_000,
  max_output: 32_768,
  cost_per_m_input: 0.5,
  cost_per_m_output: 2.0,
  capabilities: ['reasoning', 'vision'],
}

function renderEditor(overrides: Partial<Parameters<typeof ModelMetaEditor>[0]> = {}) {
  const onSave = vi.fn()
  const onCancel = vi.fn()
  render(
    <ModelMetaEditor
      modelId="proxy-model-x"
      spec={SPEC}
      onSave={onSave}
      onCancel={onCancel}
      {...overrides}
    />,
  )
  return { onSave, onCancel }
}

const get = (testId: string) => screen.getByTestId(testId) as HTMLInputElement

describe('ModelMetaEditor', () => {
  it('pre-fills the draft from the existing declaration', () => {
    renderEditor()
    expect((get('meta-editor-context').value)).toBe('198000')
    expect((get('meta-editor-max-output').value)).toBe('32768')
    expect((get('meta-editor-price-in').value)).toBe('0.5')
    expect((get('meta-editor-price-out').value)).toBe('2')
    expect((get('meta-editor-vision').checked)).toBe(true)
    expect((get('meta-editor-tools').checked)).toBe(false)
    // The "declared pricing wins" note is rendered.
    expect(screen.getByText(/Declared pricing takes precedence over the built-in catalog/)).toBeInTheDocument()
  })

  it('emits a complete merged payload on save (edit → save → payload shape)', () => {
    const { onSave } = renderEditor()
    fireEvent.change(get('meta-editor-price-in'), { target: { value: '0.75' } })
    fireEvent.change(get('meta-editor-max-output'), { target: { value: '65536' } })
    fireEvent.click(get('meta-editor-vision'))
    fireEvent.click(get('meta-editor-tools'))
    fireEvent.click(screen.getByTestId('meta-editor-save'))

    expect(onSave).toHaveBeenCalledTimes(1)
    expect(onSave).toHaveBeenCalledWith({
      id: 'proxy-model-x',
      display_name: 'Proxy X', // preserved (not an editor field)
      context_window: 198000, // untouched field re-sent as-is
      max_output: 65536,
      cost_per_m_input: 0.75,
      cost_per_m_output: 2,
      // reasoning preserved; vision unchecked; tool_use added.
      capabilities: ['reasoning', 'tool_use'],
    })
  })

  it('clears a declaration field when the input is emptied (honest fallback)', () => {
    const { onSave } = renderEditor()
    fireEvent.change(get('meta-editor-context'), { target: { value: '' } })
    fireEvent.click(screen.getByTestId('meta-editor-save'))
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({ context_window: null }),
    )
  })

  it('intercepts invalid input inline and never calls onSave', () => {
    const { onSave } = renderEditor()
    fireEvent.change(get('meta-editor-price-out'), { target: { value: '-1' } })
    fireEvent.change(get('meta-editor-max-output'), { target: { value: '0' } })
    fireEvent.click(screen.getByTestId('meta-editor-save'))

    expect(onSave).not.toHaveBeenCalled()
    expect(screen.getByTestId('meta-editor-price-out-error')).toHaveTextContent(
      'Output price must be a non-negative number',
    )
    expect(screen.getByTestId('meta-editor-max-output-error')).toHaveTextContent(
      'Max output must be a positive whole number',
    )
    expect(get('meta-editor-price-out')).toHaveAttribute('aria-invalid', 'true')
  })

  it('recovers after a fix: valid input clears the errors and saves', () => {
    const { onSave } = renderEditor()
    fireEvent.change(get('meta-editor-price-out'), { target: { value: '-1' } })
    fireEvent.click(screen.getByTestId('meta-editor-save'))
    expect(onSave).not.toHaveBeenCalled()

    fireEvent.change(get('meta-editor-price-out'), { target: { value: '3' } })
    fireEvent.click(screen.getByTestId('meta-editor-save'))
    expect(onSave).toHaveBeenCalledTimes(1)
    expect(screen.queryByTestId('meta-editor-price-out-error')).not.toBeInTheDocument()
  })

  it('cancel backs out without saving', () => {
    const { onSave, onCancel } = renderEditor()
    fireEvent.change(get('meta-editor-price-in'), { target: { value: '9' } })
    fireEvent.click(screen.getByTestId('meta-editor-cancel'))
    expect(onSave).not.toHaveBeenCalled()
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('renders from a blank slate for an undeclared row (vault-only id)', () => {
    const onSave = vi.fn()
    render(<ModelMetaEditor modelId="fresh-id" spec={null} onSave={onSave} onCancel={vi.fn()} />)
    expect(get('meta-editor-context').value).toBe('')
    fireEvent.change(get('meta-editor-context'), { target: { value: '8192' } })
    fireEvent.click(get('meta-editor-vision'))
    fireEvent.click(screen.getByTestId('meta-editor-save'))
    expect(onSave).toHaveBeenCalledWith({
      id: 'fresh-id',
      display_name: null,
      context_window: 8192,
      max_output: null,
      cost_per_m_input: null,
      cost_per_m_output: null,
      capabilities: ['vision'],
    })
  })
})
