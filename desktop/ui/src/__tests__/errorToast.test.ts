import { describe, it, expect, vi, beforeEach } from 'vitest'
import { errorMessage, toastError } from '@/lib/errorToast'

vi.mock('sonner', () => ({
  toast: { error: vi.fn() },
}))

import { toast } from 'sonner'

describe('errorMessage', () => {
  it('extracts message from Error', () => {
    expect(errorMessage(new Error('boom'))).toBe('boom')
  })

  it('returns string errors as-is', () => {
    expect(errorMessage('network down')).toBe('network down')
  })

  it('extracts .message from Error-like objects', () => {
    expect(errorMessage({ message: 'tauri rejected' })).toBe('tauri rejected')
  })

  it('stringifies unknown shapes', () => {
    expect(errorMessage({ code: 42 })).toBe('[object Object]')
    expect(errorMessage(42)).toBe('42')
  })
})

describe('toastError', () => {
  beforeEach(() => {
    vi.mocked(toast.error).mockReset()
  })

  it('passes translation as title and cause as description', () => {
    toastError('Save failed', new Error('permission denied'))
    expect(toast.error).toHaveBeenCalledWith('Save failed', {
      description: 'permission denied',
    })
  })

  it('normalises non-Error throws into description', () => {
    toastError('Save failed', 'string error')
    expect(toast.error).toHaveBeenCalledWith('Save failed', {
      description: 'string error',
    })
  })
})
