import { useEffect, useRef } from 'react'
import { listen, type UnlistenFn, type EventCallback } from '@tauri-apps/api/event'

export function useTauriEvent<T>(event: string, handler: EventCallback<T>) {
  const handlerRef = useRef(handler)
  handlerRef.current = handler

  useEffect(() => {
    let unlisten: UnlistenFn | undefined
    let cancelled = false
    listen<T>(event, (e) => {
      if (!cancelled) handlerRef.current(e)
    })
      .then(fn => {
        if (cancelled) {
          fn()
        } else {
          unlisten = fn
        }
      })
      .catch(() => {})
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [event])
}
