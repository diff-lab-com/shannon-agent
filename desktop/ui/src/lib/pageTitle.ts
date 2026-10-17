// H3 (导航批, plan §2.3-3): single source for the page-context title.
// The Header's 14px context title and the window `document.title`
// (useDocumentTitle) both read this mapping so the two surfaces can never
// drift. Keys are the `header.title.*` i18n ids — no separate title
// vocabulary exists. Longest/most-specific prefixes first: `/opc/task`
// must win over `/opc`.
const TITLE_MAP: [string, string][] = [
  ['/opc/task', 'header.title.opcTask'],
  ['/opc', 'header.title.opc'],
  ['/settings', 'header.title.settings'],
  ['/tasks', 'header.title.tasks'],
  ['/extensions', 'header.title.extensions'],
  ['/memory', 'header.title.memory'],
  ['/triage', 'header.title.triage'],
  ['/quickfix', 'header.title.quickfix'],
  ['/welcome', 'header.title.welcome'],
  ['/usage', 'header.title.usage'],
  ['/files', 'header.title.files'],
  ['/timeline', 'header.title.timeline'],
  ['/chat', 'header.title.chat'],
]

export function getTitleKey(pathname: string): string {
  for (const [prefix, key] of TITLE_MAP) {
    if (pathname.includes(prefix)) return key
  }
  return 'header.title.chat'
}
