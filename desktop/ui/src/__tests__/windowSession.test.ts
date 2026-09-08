import { describe, it, expect } from 'vitest'
import {
  parseWindowSession,
  isEventForCurrentWindow,
  WINDOW_SESSION_PARAM,
} from '@/lib/windowSession'

// P1-1 — session multi-window helpers: URL parsing + per-window event
// scoping. The backend opens windows at `/?windowSession=<uuid>`; the
// main window has no such param.

const UUID_A = '7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1'

describe('parseWindowSession', () => {
  it('returns null when there is no query string (main window)', () => {
    expect(parseWindowSession('')).toBeNull()
    expect(parseWindowSession('/')).toBeNull()
  })

  it('parses a valid uuid param', () => {
    expect(parseWindowSession(`?${WINDOW_SESSION_PARAM}=${UUID_A}`)).toBe(UUID_A)
  })

  it('trims and lower-cases the value', () => {
    expect(
      parseWindowSession(`?${WINDOW_SESSION_PARAM}=${UUID_A.toUpperCase()}%20`),
    ).toBe(UUID_A)
  })

  it('returns null for non-uuid values — a malformed param must never switch sessions', () => {
    expect(parseWindowSession(`?${WINDOW_SESSION_PARAM}=not-a-uuid`)).toBeNull()
    expect(parseWindowSession(`?${WINDOW_SESSION_PARAM}=`)).toBeNull()
    expect(parseWindowSession('/?something=else')).toBeNull()
  })
})

describe('isEventForCurrentWindow', () => {
  it('main window accepts everything (unchanged behavior)', () => {
    expect(isEventForCurrentWindow(UUID_A, null)).toBe(true)
    expect(isEventForCurrentWindow('other-session', null)).toBe(true)
    expect(isEventForCurrentWindow(undefined, null)).toBe(true)
  })

  it('session window accepts its own session', () => {
    expect(isEventForCurrentWindow(UUID_A, UUID_A)).toBe(true)
  })

  it('session window drops other sessions (cross-talk filter)', () => {
    expect(isEventForCurrentWindow('00000000-0000-0000-0000-000000000000', UUID_A)).toBe(false)
  })

  it('session window accepts legacy payloads without session_id (graceful degradation)', () => {
    expect(isEventForCurrentWindow(undefined, UUID_A)).toBe(true)
    expect(isEventForCurrentWindow(null, UUID_A)).toBe(true)
  })
})
