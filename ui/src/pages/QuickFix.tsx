// QuickFix page — developer-facing launcher for LspQuickFixPanel.
//
// Paste a file path + line + message + language, get the code actions
// the language server offers at that location.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import LspQuickFixPanel, {
  type LspQuickFixDiagnostic,
} from '@/components/lsp/LspQuickFixPanel'

const DEFAULT_DIAG: LspQuickFixDiagnostic = {
  file_path: '',
  start_line: 0,
  start_character: 0,
  end_line: 0,
  end_character: 1,
  message: '',
  language_id: 'rust',
}

const LANGUAGES = ['rust', 'typescript', 'typescriptreact', 'javascript', 'go', 'python']

export default function QuickFix() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [diag, setDiag] = useState<LspQuickFixDiagnostic>(DEFAULT_DIAG)
  const [submitted, setSubmitted] = useState<LspQuickFixDiagnostic | null>(null)

  const canSubmit = diag.file_path.trim() !== '' && diag.message.trim() !== ''

  const onSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    setSubmitted({ ...diag })
  }

  return (
    <div className="max-w-3xl mx-auto p-md flex flex-col gap-md">
      <header>
        <h2 className="font-headline-md text-on-surface">{t('quickFix.title')}</h2>
        <p className="font-label-sm text-on-surface-variant mt-xs">
          {t('quickFix.subtitle')}
        </p>
      </header>

      <form
        onSubmit={onSubmit}
        className="bg-surface-container-lowest rounded-2xl p-md border border-outline-variant/30 shadow-sm flex flex-col gap-sm"
      >
        <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
          {t('quickFix.filePath')}
          <input
            type="text"
            value={diag.file_path}
            onChange={(e) => setDiag({ ...diag, file_path: e.target.value })}
            placeholder="/abs/path/to/src/lib.rs"
            className="font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          />
        </label>

        <div className="grid grid-cols-2 gap-sm">
          <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
            {t('quickFix.startLine')}
            <input
              type="number"
              min={0}
              value={diag.start_line}
              onChange={(e) =>
                setDiag({ ...diag, start_line: Number(e.target.value) || 0 })
              }
              className="font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            />
          </label>
          <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
            {t('quickFix.startChar')}
            <input
              type="number"
              min={0}
              value={diag.start_character}
              onChange={(e) =>
                setDiag({ ...diag, start_character: Number(e.target.value) || 0 })
              }
              className="font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            />
          </label>
        </div>

        <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
          {t('quickFix.message')}
          <input
            type="text"
            value={diag.message}
            onChange={(e) => setDiag({ ...diag, message: e.target.value })}
            placeholder="unused variable: `x`"
            className="font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          />
        </label>

        <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
          {t('quickFix.language')}
          <select
            value={diag.language_id}
            onChange={(e) => setDiag({ ...diag, language_id: e.target.value })}
            className="font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          >
            {LANGUAGES.map((l) => (
              <option key={l} value={l}>
                {l}
              </option>
            ))}
          </select>
        </label>

        <button
          type="submit"
          disabled={!canSubmit}
          className="self-start font-label-md bg-primary text-on-primary rounded-full px-md py-sm cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        >
          {t('quickFix.askLSP')}
        </button>
      </form>

      {submitted ? (
        <LspQuickFixPanel
          key={`${submitted.file_path}-${submitted.start_line}`}
          diagnostic={submitted}
          onApplied={() => {
            // no-op — the panel shows its own confirmation
          }}
          onClose={() => setSubmitted(null)}
        />
      ) : null}
    </div>
  )
}
