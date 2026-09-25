/**
 * §4 P0-B — file path detection: the conservative token heuristic, working
 * dir resolution, tool-input extraction, and the chip's click routing.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  basenameOf,
  extractToolInputPath,
  getActiveWorkingDir,
  looksLikeFilePath,
  resolveFileRefPath,
  setActiveWorkingDir,
} from '@/lib/fileRefs'
import { openFileRef } from '@/lib/openFileRef'

describe('looksLikeFilePath', () => {
  it('accepts extension-bearing tokens anywhere', () => {
    expect(looksLikeFilePath('src/main.rs')).toBe(true)
    expect(looksLikeFilePath('README.md')).toBe(true)
    expect(looksLikeFilePath('Cargo.toml')).toBe(true)
    expect(looksLikeFilePath('docs/plan.design.md')).toBe(true)
  })

  it('accepts extensionless tokens only with explicit path prefixes', () => {
    expect(looksLikeFilePath('/usr/local/bin')).toBe(true)
    expect(looksLikeFilePath('./scripts')).toBe(true)
    expect(looksLikeFilePath('../out')).toBe(true)
    expect(looksLikeFilePath('~/notes')).toBe(true)
    expect(looksLikeFilePath('Dockerfile')).toBe(false)
    expect(looksLikeFilePath('src/handlers')).toBe(false)
  })

  it('rejects URLs, schemes, whitespace and junk', () => {
    expect(looksLikeFilePath('https://example.com/a.md')).toBe(false)
    expect(looksLikeFilePath('ftp://x/y.rs')).toBe(false)
    expect(looksLikeFilePath('javascript:alert(1)')).toBe(false)
    expect(looksLikeFilePath('src/a b.rs')).toBe(false)
    expect(looksLikeFilePath('')).toBe(false)
    expect(looksLikeFilePath('3 + 4 = 7.md?')).toBe(false)
  })
})

describe('resolveFileRefPath', () => {
  it('passes absolute paths through', () => {
    expect(resolveFileRefPath('/etc/hosts', null)).toBe('/etc/hosts')
  })

  it('joins relative paths onto the working dir', () => {
    expect(resolveFileRefPath('src/main.rs', '/proj')).toBe('/proj/src/main.rs')
    expect(resolveFileRefPath('src/main.rs', '/proj/')).toBe('/proj/src/main.rs')
  })

  it('resolves ./ and ../ segments lexically', () => {
    expect(resolveFileRefPath('./a/b.rs', '/proj')).toBe('/proj/a/b.rs')
    expect(resolveFileRefPath('../shared/x.ts', '/proj/app')).toBe('/proj/shared/x.ts')
  })

  it('cannot resolve ~ (no UI home hint) or relative paths without a working dir', () => {
    expect(resolveFileRefPath('~/notes.md', null)).toBeNull()
    expect(resolveFileRefPath('~/notes.md', '/proj')).toBeNull()
    expect(resolveFileRefPath('src/a.rs', null)).toBeNull()
  })
})

describe('extractToolInputPath', () => {
  it('picks the first path-ish field', () => {
    expect(extractToolInputPath({ path: '/a/b.rs' })).toBe('/a/b.rs')
    expect(extractToolInputPath({ file_path: '/a/b.rs' })).toBe('/a/b.rs')
    expect(extractToolInputPath({ notebook_path: '/a.ipynb' })).toBe('/a.ipynb')
    expect(extractToolInputPath({ command: 'ls' })).toBeNull()
    expect(extractToolInputPath(null)).toBeNull()
  })
})

describe('working dir ref', () => {
  afterEach(() => setActiveWorkingDir(null))

  it('stores the active session working dir', () => {
    expect(getActiveWorkingDir()).toBeNull()
    setActiveWorkingDir('/proj')
    expect(getActiveWorkingDir()).toBe('/proj')
  })
})

describe('basenameOf', () => {
  it('returns the last segment', () => {
    expect(basenameOf('/a/b/c.rs')).toBe('c.rs')
    expect(basenameOf('c.rs')).toBe('c.rs')
  })
})

describe('openFileRef routing', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('routes artifact extensions to shannon:open-artifact-file', () => {
    const spy = vi.fn()
    window.addEventListener('shannon:open-artifact-file', spy)
    const surface = openFileRef('/proj/report.md')
    window.removeEventListener('shannon:open-artifact-file', spy)
    expect(surface).toBe('artifact')
    expect(spy).toHaveBeenCalledTimes(1)
    expect((spy.mock.calls[0]![0] as CustomEvent).detail.path).toBe('/proj/report.md')
  })

  it('routes images to the artifact host and code to the editor event', () => {
    const artifactSpy = vi.fn()
    const codeSpy = vi.fn()
    window.addEventListener('shannon:open-artifact-file', artifactSpy)
    window.addEventListener('shannon:open-code-file', codeSpy)
    expect(openFileRef('/proj/diagram.png')).toBe('artifact')
    expect(openFileRef('/proj/main.rs')).toBe('editor')
    window.removeEventListener('shannon:open-artifact-file', artifactSpy)
    window.removeEventListener('shannon:open-code-file', codeSpy)
    expect(artifactSpy).toHaveBeenCalledTimes(1)
    expect(codeSpy).toHaveBeenCalledTimes(1)
  })
})
