// LspQuickFixPanel — surfaces LSP code actions for a single diagnostic.
//
// Caller passes a diagnostic (file path, line range, message) plus the LSP
// server command to spawn (e.g. rust-analyzer). The panel calls
// `lspCodeActions`, shows the results as buttons, and applies the chosen
// action's workspace edit on click.

import { useEffect, useState, useCallback } from 'react'
import { useIntl } from 'react-intl'
import * as api from '@/lib/tauri-api'
import type { CodeActionDto } from '@/lib/tauri-api'

export interface LspQuickFixDiagnostic {
  file_path: string
  start_line: number
  start_character: number
  end_line: number
  end_character: number
  message: string
  language_id: string
}

export interface LspQuickFixPanelProps {
  diagnostic: LspQuickFixDiagnostic
  server_cmd?: string
  server_args?: string[]
  onApplied?: () => void
  onClose?: () => void
}

const DEFAULT_SERVERS: Record<string, { cmd: string; args: string[] }> = {
  rust: { cmd: 'rust-analyzer', args: [] },
  typescript: { cmd: 'typescript-language-server', args: ['--stdio'] },
  typescriptreact: { cmd: 'typescript-language-server', args: ['--stdio'] },
  javascript: { cmd: 'typescript-language-server', args: ['--stdio'] },
  go: { cmd: 'gopls', args: [] },
  python: { cmd: 'pylsp', args: [] },
}

const EMPTY_SERVER = { cmd: '', args: [] as string[] }

export default function LspQuickFixPanel({
  diagnostic,
  server_cmd,
  server_args,
  onApplied,
  onClose,
}: LspQuickFixPanelProps) {
  const intl = useIntl()
  const server = DEFAULT_SERVERS[diagnostic.language_id] ?? EMPTY_SERVER
  const cmd = server_cmd ?? server.cmd
  const args = server_args ?? server.args

  const [actions, setActions] = useState<CodeActionDto[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [applying, setApplying] = useState<string | null>(null)
  const [lastApplied, setLastApplied] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    if (!cmd) {
      setError(intl.formatMessage({ id: 'lsp.quickFix.noLspConfigured' }, { language: diagnostic.language_id }))
      setActions([])
      return
    }
    setLoading(true)
    setError(null)
    try {
      const res = await api.lspCodeActions({
        file_path: diagnostic.file_path,
        server_cmd: cmd,
        server_args: args,
        start_line: diagnostic.start_line,
        start_character: diagnostic.start_character,
        end_line: diagnostic.end_line,
        end_character: diagnostic.end_character,
        language_id: diagnostic.language_id,
        diagnostic_messages: [diagnostic.message],
      })
      setActions(res.actions)
    } catch (e) {
      setError(String(e))
      setActions([])
    } finally {
      setLoading(false)
    }
  }, [cmd, args, diagnostic, intl])

  useEffect(() => { fetch() }, [fetch])

  const onApply = async (a: CodeActionDto) => {
    if (!a.edit) {
      setError(intl.formatMessage({ id: 'lsp.quickFix.noWorkspaceEdit' }, { title: a.title }))
      return
    }
    setApplying(a.title)
    setError(null)
    try {
      const count = await api.applyCodeAction(a.edit)
      setLastApplied(intl.formatMessage({ id: 'lsp.quickFix.applies' }, { title: a.title, count }))
      onApplied?.()
    } catch (e) {
      setError(intl.formatMessage({ id: 'lsp.quickFix.applyFailed' }, { error: e instanceof Error ? e.message : String(e) }))
    } finally {
      setApplying(null)
    }
  }

  return (
    <div
      className="bg-surface-container-lowest rounded-2xl p-md border border-outline-variant/30 shadow-sm flex flex-col gap-sm"
      role="region"
      aria-label="LSP quick-fix panel"
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-sm min-w-0">
          <span className="material-symbols-outlined text-[18px] text-primary">build</span>
          <h4 className="font-label-md text-on-surface truncate">{intl.formatMessage({ id: 'lsp.quickFix.title' })}</h4>
        </div>
        <div className="flex items-center gap-xs">
          <button
            type="button"
            onClick={fetch}
            disabled={loading || !cmd}
            className="font-label-sm text-primary hover:bg-primary/10 rounded px-xs py-1 cursor-pointer flex items-center gap-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40"
            aria-label={intl.formatMessage({ id: 'lsp.quickFix.refresh.aria' })}
          >
            <span className="material-symbols-outlined text-[14px]">{loading ? 'hourglass_top' : 'refresh'}</span>
          </button>
          {onClose ? (
            <button
              type="button"
              onClick={onClose}
              aria-label={intl.formatMessage({ id: 'lsp.quickFix.close.aria' })}
              className="text-on-surface-variant hover:bg-surface-container-high rounded-full p-xs cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            >
              <span className="material-symbols-outlined icon-sm">close</span>
            </button>
          ) : null}
        </div>
      </div>

      <p className="font-label-sm text-[11px] text-on-surface-variant line-clamp-2">
        <code className="font-mono bg-surface-container-low px-1 rounded">{diagnostic.file_path.split('/').pop()}</code>
        :{diagnostic.start_line + 1}:{diagnostic.start_character + 1} — {diagnostic.message}
      </p>

      {error ? (
        <div className="bg-error/10 border border-error/30 rounded-lg p-sm font-label-sm text-error flex items-start gap-sm" role="alert">
          <span className="material-symbols-outlined text-[14px] mt-0.5">error</span>
          <span className="flex-1 break-words">{error}</span>
        </div>
      ) : null}

      {lastApplied ? (
        <div className="bg-tertiary/10 border border-tertiary/30 rounded-lg p-sm font-label-sm text-tertiary flex items-start gap-sm">
          <span className="material-symbols-outlined text-[14px] mt-0.5">check_circle</span>
          <span className="flex-1">{intl.formatMessage({ id: 'lsp.quickFix.applied' }, { result: lastApplied })}</span>
        </div>
      ) : null}

      {loading ? (
        <p className="font-label-sm text-on-surface-variant text-center py-sm">{intl.formatMessage({ id: 'lsp.quickFix.asking' }, { cmd })}</p>
      ) : actions.length === 0 && !error ? (
        <p className="font-label-sm text-on-surface-variant italic">{intl.formatMessage({ id: 'lsp.quickFix.noQuickFixes' })}</p>
      ) : (
        <ul className="flex flex-col gap-xs">
          {actions.map((a, idx) => (
            <li key={`${a.title}-${idx}`}>
              <button
                type="button"
                disabled={applying !== null || !a.edit}
                onClick={() => onApply(a)}
                className={`w-full text-left flex items-center gap-sm px-sm py-sm rounded-lg border font-label-md ${
                  a.is_preferred
                    ? 'border-tertiary/40 bg-tertiary/10 text-on-surface hover:bg-tertiary/20'
                    : 'border-outline-variant/30 bg-surface-container-low text-on-surface hover:bg-surface-container-high'
                } disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 cursor-pointer`}
              >
                <span className="material-symbols-outlined text-[14px] text-primary">
                  {applying === a.title ? 'hourglass_top' : a.is_preferred ? 'auto_awesome' : 'healing'}
                </span>
                <span className="flex-1 truncate">{a.title}</span>
                {a.kind ? (
                  <span className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wide">
                    {a.kind.replace('quickfix.', '').replace('refactor.', '')}
                  </span>
                ) : null}
              </button>
            </li>
          ))}
        </ul>
      )}

      <p className="font-label-sm text-[10px] text-on-surface-variant mt-xs">
        {intl.formatMessage({ id: 'lsp.quickFix.spawnsServer' }, { cmd })}
      </p>
    </div>
  )
}
