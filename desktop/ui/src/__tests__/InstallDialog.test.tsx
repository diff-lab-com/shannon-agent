// X2 安装时信任卡 — the InstallDialog must show WHAT an install will enable
// before the user confirms, derived strictly from data the catalog entry
// carries (source, package metadata, description). Fields the manifest does
// not model (MCP tool lists, runtime permission categories) must NOT be
// invented — the after-install fallback line covers them instead.

import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import InstallDialog from '@/components/extensions/InstallDialog'
import type { CatalogEntry } from '@/types'

const entry = (overrides: Partial<CatalogEntry> = {}): CatalogEntry => ({
  id: 'gh:test/plugin',
  kind: 'skill',
  name: 'Test Plugin',
  description: 'A test plugin for verification.',
  author: 'Test Author',
  version: '1.0.0',
  homepage_url: null,
  license: 'MIT',
  stars: 0,
  last_updated: null,
  source: { type: 'git_hub_repo', repo: 'test/plugin', ref_: 'main' },
  trust: 'community',
  metadata: {},
  tags: [],
  ...overrides,
})

function renderDialog(spec: CatalogEntry) {
  return render(
    <MemoryRouter>
      <InstallDialog entry={spec} open onClose={vi.fn()} onInstalled={vi.fn()} />
    </MemoryRouter>,
  )
}

describe('InstallDialog — X2 trust card', () => {
  it('shows the will-be-enabled card with registry source + local-command capability for an mcp_registry entry', () => {
    renderDialog(
      entry({
        kind: 'mcp',
        name: 'github-mcp',
        description: '',
        source: { type: 'mcp_registry', publisher: 'acme' },
        metadata: { package: { type: 'npm', name: '@acme/github-mcp' } },
      }),
    )
    const card = screen.getByTestId('install-trust-card')
    expect(card).toBeInTheDocument()
    expect(screen.getByText('Will be enabled')).toBeInTheDocument()
    expect(screen.getByText(/MCP registry · publisher acme/)).toBeInTheDocument()
    expect(screen.getByTestId('trust-capability-command')).toHaveTextContent(
      'Runs a local command',
    )
    // Install-time network fetch is derivable from a remote registry source.
    expect(screen.getByText('Fetches from the network at install time')).toBeInTheDocument()
    // Fallback for unmodelled fields (tool list): the after-install pointer.
    expect(
      screen.getByText(/Details such as tool lists appear in the Installed tab/),
    ).toBeInTheDocument()
    // X2: confirm button carries authorization wording.
    expect(screen.getByRole('button', { name: /Install & authorize/ })).toBeEnabled()
  })

  it('shows container capability for docker packages', () => {
    renderDialog(
      entry({
        kind: 'mcp',
        name: 'docker-mcp',
        description: '',
        source: { type: 'mcp_registry', publisher: 'acme' },
        metadata: { package: { type: 'docker', name: 'acme/mcp:latest' } },
      }),
    )
    expect(screen.getByText('Runs a local command')).toBeInTheDocument()
    expect(screen.getByText('Runs in a Docker container')).toBeInTheDocument()
  })

  it('shows the GitHub source and the skill trigger description for a git_hub_repo skill', () => {
    renderDialog(
      entry({
        kind: 'skill',
        name: 'Deploy Helper',
        description: 'Runs the deploy checklist when asked.',
        source: { type: 'git_hub_repo', repo: 'test/skill', ref_: 'main' },
      }),
    )
    expect(screen.getByText('GitHub repository')).toBeInTheDocument()
    // The repo appears in both the trust card and the GitHub body.
    expect(screen.getAllByText('test/skill').length).toBeGreaterThan(0)
    expect(screen.getByText('Trigger description')).toBeInTheDocument()
    expect(
      screen.getAllByText(/Runs the deploy checklist when asked\./).length,
    ).toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: /Install & authorize/ })).toBeInTheDocument()
  })

  it('lists OAuth scope capability for featured vendors', () => {
    renderDialog(
      entry({
        kind: 'mcp',
        name: 'vendor-mcp',
        description: '',
        source: { type: 'featured_vendor' },
        metadata: {
          transport: 'oauth_remote',
          vendor: 'Anthropic',
          endpoint: 'https://example.com/oauth',
          scopes: ['mcp:read', 'mcp:write'],
        },
      }),
    )
    expect(screen.getByText(/Featured vendor · Anthropic/)).toBeInTheDocument()
    expect(screen.getByText('OAuth authorization scopes')).toBeInTheDocument()
  })

  it('stays honest for a native entry: local-config source, no install button', () => {
    renderDialog(
      entry({
        kind: 'plugin',
        name: 'Native Thing',
        description: '',
        source: { type: 'native' },
      }),
    )
    expect(screen.getByText('Local configuration')).toBeInTheDocument()
    // Fallback body routes to the dedicated tab — nothing installs here, so
    // there must be no "Install & authorize" confirm button.
    expect(screen.queryByRole('button', { name: /Install & authorize/ })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Open tab/ })).toBeInTheDocument()
  })
})
