import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { useApp } from '@/context/AppContext'

interface ShortcutMap {
  [key: string]: () => void
}

export function useKeyboardShortcuts(onTogglePalette?: () => void, onToggleHelp?: () => void) {
  const navigate = useNavigate()
  const { cancelQuery, isQuerying } = useApp()

  useEffect(() => {
    const shortcuts: ShortcutMap = {
      'mod+n': () => navigate('/chat'),
      'mod+shift+n': () => navigate('/chat'),
      'mod+k': () => onTogglePalette?.(),
      'mod+d': () => window.dispatchEvent(new Event('shannon:change-wd')),
      'mod+/': () => {
        const sidebar = document.querySelector('[data-sidebar]')
        sidebar?.classList.toggle('collapsed')
      },
      'mod+1': () => navigate('/chat'),
      'mod+2': () => navigate('/tasks'),
      '?': () => onToggleHelp?.(),
      'escape': () => {
        if (isQuerying) cancelQuery()
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
  }, [navigate, cancelQuery, isQuerying, onTogglePalette, onToggleHelp])
}
