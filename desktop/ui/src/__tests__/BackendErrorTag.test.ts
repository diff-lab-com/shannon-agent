// R2-P2-2 — the backend error tag protocol's frontend half: a known kind
// localizes, an unknown kind and an untagged string fall back to the
// original text verbatim.

import { describe, it, expect, vi } from 'vitest'
import { describeBackendError, parseBackendErrorTag } from '@/lib/backendError'

const messageFor = vi.fn((id: string) => `localized:${id}`)

describe('parseBackendErrorTag', () => {
  it('splits kind and original text on the first separator', () => {
    expect(parseBackendErrorTag('shannon-error:no_working_dir|No working directory is set')).toEqual({
      kind: 'no_working_dir',
      original: 'No working directory is set',
    })
  })

  it('keeps pipes inside the original text part of the text', () => {
    expect(parseBackendErrorTag('shannon-error:goal_run_active|pause | stop it')).toEqual({
      kind: 'goal_run_active',
      original: 'pause | stop it',
    })
  })

  it('returns null for untagged errors', () => {
    expect(parseBackendErrorTag('error sending request')).toBeNull()
  })
})

describe('describeBackendError', () => {
  it('localizes a known kind', () => {
    expect(
      describeBackendError(
        'shannon-error:no_working_dir|No working directory is set — choose one in Settings',
        messageFor,
      ),
    ).toBe('localized:chat.error.backend.noWorkingDir')
  })

  it('falls back to the original text for an unknown kind', () => {
    const raw = 'shannon-error:brand_new_kind|Some future error'
    expect(describeBackendError(raw, messageFor)).toBe('Some future error')
  })

  it('passes untagged errors through untouched', () => {
    expect(describeBackendError('error sending request', messageFor)).toBe('error sending request')
  })
})
