// 材质「玻璃强度」(design-parity R1 2026-10-08, 12-settings-appearance §d):
// Standard vs Reduced glass. Reduced puts the persistent `reduce-glass`
// class on <html>; index.css resolves it to the same solid fill the OS-level
// prefers-reduced-transparency fallback uses. Persistence mirrors the
// theme/density keys' localStorage pattern.

const REDUCE_GLASS_KEY = 'shannon.reduceGlass'
/** Class applied to documentElement while Reduced is on. */
export const REDUCE_GLASS_CLASS = 'reduce-glass'

export function readReduceGlass(): boolean {
  try {
    return localStorage.getItem(REDUCE_GLASS_KEY) === '1'
  } catch {
    return false
  }
}

export function applyReduceGlass(reduced: boolean) {
  try {
    document.documentElement.classList.toggle(REDUCE_GLASS_CLASS, reduced)
  } catch {
    /* noop */
  }
}

export function setReduceGlass(reduced: boolean) {
  try {
    if (reduced) localStorage.setItem(REDUCE_GLASS_KEY, '1')
    else localStorage.removeItem(REDUCE_GLASS_KEY)
  } catch {
    /* noop */
  }
  applyReduceGlass(reduced)
}

/** Boot-time: apply the persisted preference before first paint (main.tsx). */
export function initReduceGlass() {
  applyReduceGlass(readReduceGlass())
}
