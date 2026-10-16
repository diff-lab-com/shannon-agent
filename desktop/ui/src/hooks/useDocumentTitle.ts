// H3 (导航批, plan §2.3-3): keep the window `document.title` in sync with
// the route — `${page context} · Shannon Code`. Competitor convention
// (Notion/Obsidian do this; Slack/Discord's fixed title is a standing
// complaint). The page context is the SAME derivation the Header's 14px
// title uses (U2: session title on /chat, `header.title.*` page name
// elsewhere) via the shared lib/pageTitle mapping — one source, two
// surfaces, no drift. Mounted once in Layout (next to the Header), never
// per page, so lazy page chunks can't duplicate or drop it.
//
// Window mode note (P1-1): Layout separately drives the NATIVE window
// title (getCurrentWindow().setTitle) from the session title; this hook
// only maintains the webview-level document.title and does not fight it.

import { useEffect } from 'react'
import { useIntl } from 'react-intl'
import { useLocation } from 'react-router-dom'
import { useSessions } from '@/context/SessionContext'
import { getTitleKey } from '@/lib/pageTitle'

export function useDocumentTitle() {
  const intl = useIntl()
  const location = useLocation()
  const { sessions, currentSessionId } = useSessions()

  useEffect(() => {
    // Mirror of the Header's U2 title logic (Header.tsx): the current
    // session's title on /chat when one is known, the fixed page title
    // otherwise (which on /chat means `header.title.chat`).
    const isChat = location.pathname.startsWith('/chat')
    const chatSession = sessions.find(s => s.id === currentSessionId)
    const context = isChat && chatSession?.title
      ? chatSession.title
      : intl.formatMessage({ id: getTitleKey(location.pathname) })
    // `app.brandName` ("Shannon Code") is the existing brand key — no new
    // title vocabulary is introduced.
    document.title = `${context} · ${intl.formatMessage({ id: 'app.brandName' })}`
  }, [location.pathname, sessions, currentSessionId, intl])
}
