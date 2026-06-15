// Drop-in replacement for @tauri-apps/api/core when running in mock mode.
// Vite alias swaps '@tauri-apps/api/core' → this module when VITE_MOCK_MODE=1.
// See vite.config.ts and src/lib/mock/README.md.
import { handlers } from './handlers'

export interface InvokeArgs {
  [key: string]: unknown
}

export async function invoke<T = unknown>(cmd: string, args?: InvokeArgs): Promise<T> {
  const handler = handlers[cmd]
  if (handler) {
    try {
      return await handler(args ?? {}) as T
    } catch (e) {
      console.warn(`[mock] handler for "${cmd}" threw:`, e)
      throw e
    }
  }
  // Surface missing handlers loudly — better UX than silent failure
  const msg = `[mock] unhandled Tauri command: "${cmd}". Add it to src/lib/mock/handlers.ts.`
  console.error(msg)
  throw new Error(msg)
}

// Re-export other things from @tauri-apps/api/core that the app might use,
// falling back to the real module when not mocking. This file is only loaded
// when the alias is active, so we just stub the surface we know about.
export async function convertFileSrc(filePath: string): Promise<string> {
  // In dev (file:// or http://) just return the path — assets resolve locally
  return `file://${filePath}`
}

export function transformCallback(): number {
  return Math.floor(Math.random() * 1_000_000)
}

// Install the visible DEMO MODE badge once on module load (browser only)
if (typeof window !== 'undefined' && typeof document !== 'undefined') {
  const ready = () => {
    if (document.querySelector('[data-mock-badge]')) return
    const badge = document.createElement('div')
    badge.setAttribute('data-mock-badge', '')
    badge.textContent = 'DEMO MODE'
    badge.style.cssText = [
      'position:fixed', 'bottom:12px', 'left:12px', 'z-index:9999',
      'background:#7c3aed', 'color:white', 'font:600 11px/1 system-ui, sans-serif',
      'padding:4px 10px', 'border-radius:999px', 'letter-spacing:0.05em',
      'box-shadow:0 2px 8px rgba(124,58,237,0.4)', 'pointer-events:none',
    ].join(';')
    document.body.appendChild(badge)
  }
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', ready, { once: true })
  } else {
    ready()
  }
  console.info(`[mock] Tauri invoke intercepted. ${Object.keys(handlers).length} commands mocked.`)
}
