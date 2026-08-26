import { save as saveDialog, open as openDialog } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import { buildPrintStyles } from '@/lib/printStyles'
import type { SessionInfo } from '@/types'

// Session-level actions shared by the app-sidebar session rail (Sidebar) and
// the chat page (working-dir picker). Lives in lib/ — not pages/chat/ — so
// components can use it without importing a page module.

// Render a tiny subset of Markdown (headings, paragraphs, hr, fenced code,
// **bold**, `code`) into an existing DOM node. Built with createElement +
// textContent so all user content is auto-escaped — never use innerHTML with
// raw conversation bytes.
function appendMarkdownToElement(parent: HTMLElement, md: string) {
  const doc = parent.ownerDocument
  if (!doc) return
  const lines = md.split('\n')
  let i = 0
  let inCode = false
  let codeBuffer: string[] = []

  const flushCode = () => {
    if (codeBuffer.length === 0) return
    const pre = doc.createElement('pre')
    const code = doc.createElement('code')
    code.textContent = codeBuffer.join('\n')
    pre.appendChild(code)
    parent.appendChild(pre)
    codeBuffer = []
  }

  while (i < lines.length) {
    const line = lines[i]
    if (line.startsWith('```')) {
      if (inCode) {
        flushCode()
        inCode = false
      } else {
        inCode = true
      }
      i++
      continue
    }
    if (inCode) {
      codeBuffer.push(line)
      i++
      continue
    }
    if (line.startsWith('# ')) {
      const h = doc.createElement('h1')
      h.textContent = line.slice(2)
      parent.appendChild(h)
    } else if (line.startsWith('### ')) {
      const h = doc.createElement('h3')
      h.textContent = line.slice(4)
      parent.appendChild(h)
    } else if (/^(\s*)(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      parent.appendChild(doc.createElement('hr'))
    } else if (line.trim() === '') {
      // paragraph break — skip
    } else {
      const p = doc.createElement('p')
      p.textContent = line
      parent.appendChild(p)
    }
    i++
  }
  if (inCode) flushCode()
}

// Export a single session as a Markdown file via Tauri's native save dialog.
// The browser File System Access API can't write into arbitrary OS paths,
// so we go through `saveDialog` + `saveTextFile` instead.
export async function exportSessionAsMarkdown(
  id: string,
  sessions: SessionInfo[],
  t: (id: string) => string,
): Promise<void> {
  try {
    const md = await api.exportSession(id, 'markdown')
    const session = sessions.find(s => s.id === id)
    const defaultName = `${(session?.title || t('chat.export.defaultName')).replace(/[^a-z0-9-_]+/gi, '_').slice(0, 60)}.md`
    const target = await saveDialog({ defaultPath: defaultName, filters: [{ name: t('chat.export.markdown'), extensions: ['md'] }] })
    if (!target) return // user cancelled
    await api.saveTextFile(target, md)
    toast.success(t('chat.toast.exported'), { description: target })
  } catch (e) {
    console.warn('Export failed:', e)
    toast.error(t('chat.toast.exportFailed'), { description: String(e) })
  }
}

// Open a print-friendly window with the rendered conversation. The system
// print dialog exposes "Save as PDF" on every desktop OS, which gives us
// PDF export without dragging in a PDF library. DOM is built via
// createElement + textContent so user content is auto-escaped — no string
// interpolation into HTML.
export async function printSession(
  id: string,
  sessions: SessionInfo[],
  t: (id: string) => string,
): Promise<void> {
  try {
    const md = await api.exportSession(id, 'markdown')
    const session = sessions.find(s => s.id === id)
    const title = session?.title || t('chat.export.printTitle')
    const printWindow = window.open('', '_blank', 'width=900,height=700')
    if (!printWindow) {
      toast.error(t('chat.toast.popupBlocked'), { description: t('chat.toast.popupBlocked.desc') })
      return
    }
    const doc = printWindow.document
    doc.title = title
    const style = doc.createElement('style')
    style.textContent = buildPrintStyles({ variant: 'chat' })
    doc.head.appendChild(style)
    const h1 = doc.createElement('h1')
    h1.textContent = title
    doc.body.appendChild(h1)
    appendMarkdownToElement(doc.body, md)
    printWindow.focus()
    // Give the new window a tick to lay out before opening the print dialog.
    setTimeout(() => printWindow.print(), 250)
  } catch (e) {
    console.warn('Print failed:', e)
    toast.error(t('chat.toast.printFailed'), { description: String(e) })
  }
}

// Native-folder picker that updates the active session's working directory.
export async function changeSessionWorkingDir(
  currentSessionId: string | null,
  t: (id: string) => string,
): Promise<void> {
  if (!currentSessionId) {
    toast.error(t('chat.header.workingDir.changeFailed'), { description: t('chat.header.workingDir.noSession') })
    return
  }
  try {
    const selected = await openDialog({ directory: true, multiple: false })
    if (!selected || Array.isArray(selected)) return
    await api.setSessionWorkingDir(currentSessionId, selected as string)
    toast.success(t('chat.header.workingDir.changed'), { description: selected as string })
  } catch (err) {
    toast.error(t('chat.header.workingDir.changeFailed'), { description: String(err) })
  }
}
