// CodeEditor — CodeMirror 6 wrapper that renders diagnostic squiggles.
//
// Props:
//   - value / onValueChange: controlled text content
//   - language: language id from read_source_file ("rust", "typescript", ...)
//   - diagnostics: list of { line, message, severity } to render as squiggles
//   - onDiagnosticClick: called when user clicks a squiggle in the editor
//
// Uses @codemirror/lint's `linter()` extension with a synthetic source so
// squiggles + gutter markers render with the standard CM6 look. We add a
// `domEventHandlers` extension to catch clicks on diagnostic markers and
// dispatch them to the React handler.

import { useMemo } from 'react'
import CodeMirror from '@uiw/react-codemirror'
import { EditorView } from '@codemirror/view'
import { linter, lintGutter } from '@codemirror/lint'
import { rust } from '@codemirror/lang-rust'
import { javascript } from '@codemirror/lang-javascript'
import { python } from '@codemirror/lang-python'
import { go } from '@codemirror/lang-go'
import type { Extension } from '@codemirror/state'
import type { Diagnostic as CMDiagnostic } from '@codemirror/lint'

export interface EditorDiagnostic {
  start_line: number
  start_character: number
  end_line: number
  end_character: number
  message: string
  severity: 'error' | 'warning' | 'info' | 'hint'
}

export interface CodeEditorProps {
  value: string
  onValueChange?: (next: string) => void
  language: string
  diagnostics: EditorDiagnostic[]
  onDiagnosticClick?: (diag: EditorDiagnostic) => void
  readOnly?: boolean
}

function languageExtension(languageId: string): Extension[] {
  switch (languageId) {
    case 'rust':
      return [rust()]
    case 'typescript':
    case 'typescriptreact':
    case 'javascript':
    case 'javascriptreact':
      return [javascript({ jsx: languageId.endsWith('react'), typescript: languageId.startsWith('typescript') })]
    case 'python':
      return [python()]
    case 'go':
      return [go()]
    default:
      return []
  }
}

// Build a synthetic linter fn that maps our EditorDiagnostic[] into CM's
// absolute-offset format. The linter callback receives the live EditorView
// so we can compute offsets against the current document.

export default function CodeEditor({
  value,
  onValueChange,
  language,
  diagnostics,
  onDiagnosticClick,
  readOnly = false,
}: CodeEditorProps) {
  const langExt = useMemo(() => languageExtension(language), [language])

  // Build a linter that reads current diagnostics and computes offsets
  // from the live document.
  const diagExt = useMemo(() => {
    return linter((view) => {
      const doc = view.state.doc
      const out: CMDiagnostic[] = []
      for (const d of diagnostics) {
        const startLine = Math.min(Math.max(d.start_line + 1, 1), doc.lines)
        const endLine = Math.min(Math.max(d.end_line + 1, 1), doc.lines)
        const lineStart = doc.line(startLine)
        const lineEnd = doc.line(endLine)
        const from = Math.min(
          lineStart.from + Math.max(0, d.start_character),
          doc.length,
        )
        const to = Math.min(
          Math.max(lineEnd.from + Math.max(0, d.end_character), from),
          doc.length,
        )
        out.push({
          from,
          to,
          message: d.message,
          severity: d.severity,
        })
      }
      return out
    })
  }, [diagnostics])

  // Click handler — captures clicks on diagnostic elements and dispatches
  // them to the React handler by looking up the nearest diagnostic by line.
  const clickHandler = useMemo(
    () =>
      EditorView.domEventHandlers({
        click: (event, view) => {
          if (!onDiagnosticClick) return false
          const pos = view.posAtCoords({ x: event.clientX, y: event.clientY })
          if (pos == null) return false
          const line = view.state.doc.lineAt(pos)
          const lineNum = line.number - 1 // back to 0-based
          const matched = diagnostics.find(
            (d) => lineNum >= d.start_line && lineNum <= d.end_line,
          )
          if (matched) {
            onDiagnosticClick(matched)
            return true
          }
          return false
        },
      }),
    [diagnostics, onDiagnosticClick],
  )

  const extensions = useMemo(
    () => [langExt, lintGutter(), diagExt, clickHandler],
    [langExt, diagExt, clickHandler],
  )

  return (
    <div className="rounded-2xl border border-outline-variant/30 overflow-hidden bg-surface-container-lowest">
      <CodeMirror
        value={value}
        onChange={onValueChange}
        extensions={extensions}
        readOnly={readOnly}
        basicSetup={{
          lineNumbers: true,
          foldGutter: true,
          highlightActiveLine: true,
          bracketMatching: true,
          closeBrackets: true,
          autocompletion: false,
          searchKeymap: true,
        }}
        theme="light"
        height="60vh"
      />
    </div>
  )
}
