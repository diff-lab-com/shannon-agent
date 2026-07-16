import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import RoutineTemplatesBrowser from '@/components/routines/RoutineTemplatesBrowser'
import * as api from '@/lib/tauri-api'

describe('RoutineTemplatesBrowser', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  afterEach(() => {
    cleanup()
  })

  it('renders empty state when no templates are returned', async () => {
    vi.spyOn(api, 'listRoutineTemplates').mockResolvedValue([])
    render(<RoutineTemplatesBrowser onInstantiated={() => {}} />)
    await waitFor(() => {
      expect(
        screen.getByText('No routine templates bundled with this build.'),
      ).toBeInTheDocument()
    })
  })

  it('renders every template with its name + description', async () => {
    vi.spyOn(api, 'listRoutineTemplates').mockResolvedValue([
      {
        id: 'demo-1',
        name: 'Daily summary',
        description: 'Summarize the day',
        category: 'engineering',
        prompt: 'do work',
        trigger_type: 'cron',
        cron_expr: '0 9 * * *',
        timezone: null,
      },
      {
        id: 'demo-2',
        name: 'Weekly review',
        description: 'Review the week',
        category: 'security',
        prompt: 'review code',
        trigger_type: 'interval',
        interval_secs: 86400,
        timezone: null,
      },
    ])
    render(<RoutineTemplatesBrowser onInstantiated={() => {}} />)
    await waitFor(() => {
      expect(screen.getByText('Daily summary')).toBeInTheDocument()
      expect(screen.getByText('Summarize the day')).toBeInTheDocument()
      expect(screen.getByText('Weekly review')).toBeInTheDocument()
      expect(screen.getByText('Review the week')).toBeInTheDocument()
    })
  })

  it('invokes instantiateRoutineTemplate when "Use template" is clicked', async () => {
    vi.spyOn(api, 'listRoutineTemplates').mockResolvedValue([
      {
        id: 'demo-1',
        name: 'Daily summary',
        description: 'Summarize the day',
        category: 'engineering',
        prompt: 'do work',
        trigger_type: 'cron',
        cron_expr: '0 9 * * *',
        timezone: null,
      },
    ])
    const instantiateSpy = vi
      .spyOn(api, 'instantiateRoutineTemplate')
      .mockResolvedValue({ id: 'demo-1', name: 'Daily summary' } as any)
    const cb = vi.fn()
    render(<RoutineTemplatesBrowser onInstantiated={cb} />)
    await waitFor(() => {
      expect(screen.getByText('Use template')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Use template'))
    await waitFor(() => {
      expect(instantiateSpy).toHaveBeenCalledWith('demo-1')
      expect(cb).toHaveBeenCalled()
    })
  })

  it('shows the cron expression when trigger_type is cron', async () => {
    vi.spyOn(api, 'listRoutineTemplates').mockResolvedValue([
      {
        id: 'cron-only',
        name: 'Cron template',
        description: 'has cron',
        category: 'engineering',
        prompt: 'x',
        trigger_type: 'cron',
        cron_expr: '0 9 * * 1-5',
        timezone: null,
      },
    ])
    render(<RoutineTemplatesBrowser onInstantiated={() => {}} />)
    await waitFor(() => {
      expect(screen.getByText('0 9 * * 1-5')).toBeInTheDocument()
    })
  })
})
