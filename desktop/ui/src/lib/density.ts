// P2-⑧ display density — Comfortable (default) vs Compact. Compact scales
// the shared MD3 type/spacing tokens via html[data-density] (see
// styles/tokens.css); persistence mirrors the theme keys' localStorage
// pattern.

export type Density = 'comfortable' | 'compact'

const DENSITY_KEY = 'shannon.density'

export function readDensity(): Density {
  try {
    return localStorage.getItem(DENSITY_KEY) === 'compact' ? 'compact' : 'comfortable'
  } catch { return 'comfortable' }
}

export function applyDensity(density: Density) {
  try {
    if (density === 'compact') document.documentElement.dataset.density = 'compact'
    else delete document.documentElement.dataset.density
  } catch { /* noop */ }
}

export function setDensity(density: Density) {
  try { localStorage.setItem(DENSITY_KEY, density) } catch { /* noop */ }
  applyDensity(density)
}

/** Boot-time: apply the persisted preference before first paint (main.tsx). */
export function initDensity() {
  applyDensity(readDensity())
}
