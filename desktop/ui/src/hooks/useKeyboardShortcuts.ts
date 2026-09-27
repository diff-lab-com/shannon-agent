import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { useChat } from '@/context/ChatContext'

interface ShortcutMap {
  [key: string]: () => void
}

// T5 (review P1-6): when a floating layer (modal dialog, menu, listbox) is
// open, Escape belongs to IT — closing the overlay must never also fire the
// global「cancel query」shortcut. DropdownMenu additionally stops
// propagation on its own document-level handler; this window-level check is
// the second half of the discipline and covers Base UI dialogs and the
// hand-rolled pickers, which don't expose a shared "overlay open" flag.
function overlayOwnsEscape(): boolean {
  if (typeof document === 'undefined') return false
  return document.querySelector('[role="dialog"], [role="menu"], [role="listbox"]') != null
}

export function useKeyboardShortcuts(
  onTogglePalette?: () => void,
  onToggleHelp?: () => void,
  onCreateSession?: () => void,
) {
  const navigate = useNavigate()
  const { cancelQuery, isQuerying } = useChat()

  useEffect(() => {
    const shortcuts: ShortcutMap = {
      'mod+n': () => {
        if (onCreateSession) {
          onCreateSession()
          navigate('/chat')
        } else {
          navigate('/chat')
        }
      },
      'mod+shift+n': () => navigate('/chat'),
      'mod+k': () => onTogglePalette?.(),
      'mod+d': () => window.dispatchEvent(new Event('shannon:change-wd')),
      'mod+1': () => navigate('/chat'),
      'mod+2': () => navigate('/tasks'),
      'mod+3': () => navigate('/extensions'),
      'mod+4': () => navigate('/memory'),
      // Editor is a chat-inline panel now (standalone /editor page retired,
      // audit §3.8) — reuse the shannon:* window-event pattern. B1-16: the
      // editor only mounts on /chat, so leave it first — the old dispatch-only
      // version was a dud from every other page.
      'mod+5': () => {
        navigate('/chat')
        window.dispatchEvent(new Event('shannon:open-editor'))
      },
      'mod+6': () => navigate('/settings'),
      // B1-16: the help overlay documents Ctrl+/ — implement it (same action
      // as `?`) instead of advertising a dead binding.
      'mod+/': () => onToggleHelp?.(),
      '?': () => onToggleHelp?.(),
      'escape': () => {
        if (isQuerying && !overlayOwnsEscape()) cancelQuery()
      },
    }

    const handler = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement
      if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable) return

      const mod = e.metaKey || e.ctrlKey
      const key = e.key.toLowerCase()

      if (e.key === '?' || e.key === '/') {
        const fn = shortcuts[e.key]
        if (fn && !mod) { e.preventDefault(); fn(); return }
      }

      let combo = ''
      if (mod) combo += 'mod+'
      if (e.shiftKey) combo += 'shift+'
      combo += key

      const fn = shortcuts[combo]
      if (fn) {
        e.preventDefault()
        fn()
      }
    }

    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [navigate, cancelQuery, isQuerying, onTogglePalette, onToggleHelp, onCreateSession])
}
