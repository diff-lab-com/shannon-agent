// LinkContextMenuHost — global right-click menu for external links
// (docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md §4 P0-A ⑦).
//
// Decision §5-6: both destinations are always offered, panel first. The
// host listens on `contextmenu` at the document level, so every bare
// `<a href="https://…">` in the app gets the menu without per-call-site
// wiring. Anchor / app-route / relative links keep the native menu.

import { useEffect, useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import { isExternalHttpUrl, openLink } from '@/lib/openLink'
import { focusFirstMenuItem, handleMenuKeyDown } from './menuKeyboard'

interface MenuState {
  url: string
  x: number
  y: number
}

const MENU_WIDTH = 200
const MENU_HEIGHT_ESTIMATE = 96

export function LinkContextMenuHost() {
  const intl = useIntl()
  const [menu, setMenu] = useState<MenuState | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const onContextMenu = (e: MouseEvent) => {
      const anchor = (e.target as Element | null)?.closest?.('a[href]') as HTMLAnchorElement | null
      const href = anchor?.getAttribute('href') ?? ''
      if (!anchor || !isExternalHttpUrl(href)) return
      e.preventDefault()
      setMenu({ url: href, x: e.clientX, y: e.clientY })
    }
    const close = () => setMenu(null)
    document.addEventListener('contextmenu', onContextMenu)
    window.addEventListener('blur', close)
    return () => {
      document.removeEventListener('contextmenu', onContextMenu)
      window.removeEventListener('blur', close)
    }
  }, [])

  useEffect(() => {
    if (!menu) return
    const onPointerDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenu(null)
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenu(null)
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [menu])

  // Menu keyboard semantics (review §5): focus the first item on open.
  useEffect(() => {
    if (!menu) return
    focusFirstMenuItem(menuRef.current)
  }, [menu])

  if (!menu) return null

  const itemClass =
    'flex w-full items-center gap-xs px-sm py-xs text-left font-label-md text-on-surface hover:bg-surface-container rounded-md cursor-pointer'

  return (
    <div
      ref={menuRef}
      role="menu"
      aria-label={intl.formatMessage({ id: 'link.menu.aria' })}
      data-testid="link-context-menu"
      onKeyDown={(e) => handleMenuKeyDown(e, menuRef.current, () => setMenu(null))}
      className="fixed z-modal min-w-52 rounded-lg border border-outline-variant/20 bg-surface-container-high p-xs shadow-lg animate-in fade-in zoom-in-95"
      style={{
        left: Math.max(4, Math.min(menu.x, window.innerWidth - MENU_WIDTH - 8)),
        top: Math.max(4, Math.min(menu.y, window.innerHeight - MENU_HEIGHT_ESTIMATE - 8)),
      }}
      onClick={() => setMenu(null)}
    >
      <button type="button" role="menuitem" className={itemClass} onClick={() => void openLink(menu.url, 'panel')}>
        <span className="material-symbols-outlined text-[16px]" aria-hidden="true">right_panel_open</span>
        {intl.formatMessage({ id: 'link.menu.openPanel' })}
      </button>
      <button type="button" role="menuitem" className={itemClass} onClick={() => void openLink(menu.url, 'browser')}>
        <span className="material-symbols-outlined text-[16px]" aria-hidden="true">open_in_new</span>
        {intl.formatMessage({ id: 'link.menu.openBrowser' })}
      </button>
    </div>
  )
}
