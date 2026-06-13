import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import NewTaskForm, { composePrompt } from '@/components/tasks/NewTaskForm'

describe('composePrompt', () => {
  it('returns prompt unchanged when no metadata', () => {
    expect(composePrompt('Do thing')).toBe('Do thing')
  })

  it('prepends assignee tag only', () => {
    expect(composePrompt('Do thing', 'Bot')).toBe('[Assignee: Bot] Do thing')
  })

  it('prepends priority tag only (skips low)', () => {
    expect(composePrompt('Do thing', undefined, 'high')).toBe('[Priority: high] Do thing')
  })

  it('prepends both tags in order', () => {
    expect(composePrompt('Do thing', 'Bot', 'medium')).toBe('[Assignee: Bot] [Priority: medium] Do thing')
  })

  it('omits priority tag for low', () => {
    expect(composePrompt('Do thing', 'Bot', 'low')).toBe('[Assignee: Bot] Do thing')
  })
})

describe('NewTaskForm', () => {
  it('renders prompt textarea', () => {
    render(<NewTaskForm value="" onChange={() => {}} onSubmit={() => {}} onCancel={() => {}} />)
    expect(screen.getByPlaceholderText(/Describe the task/)).toBeInTheDocument()
  })

  it('disables Create Task when prompt empty', () => {
    render(<NewTaskForm value="" onChange={() => {}} onSubmit={() => {}} onCancel={() => {}} />)
    expect(screen.getByRole('button', { name: /^Create Task$/ })).toBeDisabled()
  })

  it('enables Create Task when prompt has text', () => {
    render(<NewTaskForm value="hello" onChange={() => {}} onSubmit={() => {}} onCancel={() => {}} />)
    expect(screen.getByRole('button', { name: /^Create Task$/ })).toBeEnabled()
  })

  it('shows Add options button by default', () => {
    render(<NewTaskForm value="" onChange={() => {}} onSubmit={() => {}} onCancel={() => {}} />)
    expect(screen.getByRole('button', { name: /Add options/ })).toBeInTheDocument()
  })

  it('reveals assignee/priority fields on Add options click', () => {
    render(<NewTaskForm value="" onChange={() => {}} onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /Add options/ }))
    expect(screen.getByPlaceholderText(/Research Agent/)).toBeInTheDocument()
    expect(screen.getByDisplayValue('Low')).toBeInTheDocument()
  })

  it('passes composed prompt on submit', () => {
    const onSubmit = vi.fn()
    render(<NewTaskForm value="build it" onChange={() => {}} onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /Add options/ }))
    fireEvent.change(screen.getByPlaceholderText(/Research Agent/), { target: { value: 'Dev' } })
    fireEvent.change(screen.getByDisplayValue('Low'), { target: { value: 'high' } })
    fireEvent.click(screen.getByRole('button', { name: /^Create Task$/ }))
    expect(onSubmit).toHaveBeenCalledWith({ prompt: '[Assignee: Dev] [Priority: high] build it', assignee: 'Dev', priority: 'high' })
  })

  it('hides options panel on second toggle', () => {
    render(<NewTaskForm value="" onChange={() => {}} onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /Add options/ }))
    expect(screen.getByPlaceholderText(/Research Agent/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Hide options/ }))
    expect(screen.queryByPlaceholderText(/Research Agent/)).not.toBeInTheDocument()
  })

  it('Cancel button calls onCancel', () => {
    const onCancel = vi.fn()
    render(<NewTaskForm value="" onChange={() => {}} onSubmit={() => {}} onCancel={onCancel} />)
    fireEvent.click(screen.getByRole('button', { name: /^Cancel$/ }))
    expect(onCancel).toHaveBeenCalled()
  })
})
