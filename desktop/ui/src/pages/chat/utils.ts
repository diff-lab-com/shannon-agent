// Pure helpers used across the Chat page subtree. No React imports so this
// file stays tree-shakeable and can be imported from server-side tests if
// needed.

// Render a tiny subset of Markdown (headings, paragraphs, hr, fenced code,
// **bold**, `code`) into an existing DOM node. Built with createElement +
// textContent so all user content is auto-escaped — never use innerHTML with
// raw conversation bytes.
export function appendMarkdownToElement(parent: HTMLElement, md: string) {
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

// Collapse a long absolute path into a tail-only breadcrumb for the chip UI.
// "/home/alice/code/myproject" → "…/code/myproject"
export function formatDirBreadcrumb(full: string): string {
  const parts = full.replace(/\\/g, '/').split('/').filter(Boolean)
  if (parts.length <= 2) return full
  return '…/' + parts.slice(-2).join('/')
}

// Localized relative-date chip ("Today" / "Yesterday" / locale date).
// `t` is the intl-aware translation function (so the function stays pure
// and the parent owns i18n context).
export function formatTime(t: (id: string) => string, ts: number): string {
  const d = new Date(ts)
  const now = new Date()
  if (d.toDateString() === now.toDateString()) return t('chat.time.today')
  const yesterday = new Date(now)
  yesterday.setDate(yesterday.getDate() - 1)
  if (d.toDateString() === yesterday.toDateString()) return t('chat.time.yesterday')
  return d.toLocaleDateString()
}