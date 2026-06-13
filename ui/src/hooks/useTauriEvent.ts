import { useEffect, useRef } from 'react'
import { listen, type UnlistenFn, type EventCallback } from '@tauri-apps/api/event'

export function useTauriEvent<T>(event: string, handler: EventCallback<T>) {
  const handlerRef = useRef(handler)
  handlerRef.current = handler

  useEffect(() => {
    let unlisten: UnlistenFn | undefined
    listen<T>(event, (e) => handlerRef.current(e)).then(fn => { unlisten = fn })
    return () => { unlisten?.() }
  }, [event])
}
