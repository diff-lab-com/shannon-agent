// AddSquiggleForm — manual diagnostic form (line, start char, end char,
// severity, message). All field values + onAddSquiggle come from the
// orchestrator.

import { Button } from '@/components/ui/button'
import { SEVERITIES } from './constants'
import type { EditorDiagnostic } from '@/components/editor/CodeEditor'

interface AddSquiggleFormProps {
  t: (id: string, values?: Record<string, string | number | boolean>) => string
  newLine: number
  setNewLine: (n: number) => void
  newStartChar: number
  setNewStartChar: (n: number) => void
  newEndChar: number
  setNewEndChar: (n: number) => void
  newMessage: string
  setNewMessage: (s: string) => void
  newSeverity: EditorDiagnostic['severity']
  setNewSeverity: (s: EditorDiagnostic['severity']) => void
  onAddSquiggle: (e: React.FormEvent) => void
}

export default function AddSquiggleForm({
  t,
  newLine,
  setNewLine,
  newStartChar,
  setNewStartChar,
  newEndChar,
  setNewEndChar,
  newMessage,
  setNewMessage,
  newSeverity,
  setNewSeverity,
  onAddSquiggle,
}: AddSquiggleFormProps) {
  return (
    <form
      onSubmit={onAddSquiggle}
      className="bg-surface-container-lowest rounded-2xl p-md border border-outline-variant/30 shadow-sm flex flex-col gap-sm"
    >
      <h3 className="font-label-md text-on-surface">{t('editor.addSquiggle')}</h3>
      <div className="grid grid-cols-4 gap-sm">
        <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
          {t('editor.line')}
          <input
            type="number"
            min={0}
            value={newLine}
            onChange={(e) => setNewLine(Number(e.target.value) || 0)}
            className="font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          />
        </label>
        <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
          {t('editor.startChar')}
          <input
            type="number"
            min={0}
            value={newStartChar}
            onChange={(e) => setNewStartChar(Number(e.target.value) || 0)}
            className="font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          />
        </label>
        <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
          {t('editor.endChar')}
          <input
            type="number"
            min={0}
            value={newEndChar}
            onChange={(e) => setNewEndChar(Number(e.target.value) || 0)}
            className="font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          />
        </label>
        <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
          {t('editor.severity')}
          <select
            value={newSeverity}
            onChange={(e) =>
              setNewSeverity(e.target.value as EditorDiagnostic['severity'])
            }
            className="font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          >
            {SEVERITIES.map((s) => (
              <option key={s} value={s}>
                {t(`editor.severity.${s}`)}
              </option>
            ))}
          </select>
        </label>
      </div>
      <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
        {t('editor.message')}
        <input
          type="text"
          value={newMessage}
          onChange={(e) => setNewMessage(e.target.value)}
          placeholder={t('editor.message.placeholder')}
          className="font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        />
      </label>
      <Button
        type="submit"
        disabled={!newMessage.trim()}
        className="self-start font-label-md bg-primary text-on-primary rounded-lg px-md py-sm cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
      >
        {t('editor.addSquiggleBtn')}
      </Button>
    </form>
  )
}