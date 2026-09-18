// P2-⑧ display density — Comfortable (default) vs Compact. Compact scales
// the shared MD3 type/spacing tokens via html[data-density] (see
// styles/tokens.css); persistence mirrors the theme keys' localStorage
// pattern.

export type Density = 'comfortable' | 'compact'

const DENSITY_KEY = 'shannon.density'
/** D6: `null` = follow the sidebar mode (Advanced→Compact, Simple→Comfortable);
 *  an explicit user choice persists as 'comfortable'|'compact' and wins. */
const DENSITY_PREF_KEY = 'shannon.density.pref'

export type DensityPref = Density | 'auto'

export function readDensityPref(): DensityPref {
  try {
    const raw = localStorage.getItem(DENSITY_PREF_KEY)
    if (raw === 'compact' || raw === 'comfortable') return raw
  } catch { /* noop */ }
  return 'auto'
}

export function setDensityPref(pref: DensityPref) {
  try {
    // 'auto' removes the override so the sidebar-mode default applies again.
    if (pref === 'auto') localStorage.removeItem(DENSITY_PREF_KEY)
    else localStorage.setItem(DENSITY_PREF_KEY, pref)
  } catch { /* noop */ }
}

/** Sidebar mode at decision time ('advanced' | 'simple'); injected by the
 *  caller (Layout owns the mode) to avoid a circular import. */
export function resolveDensity(pref: DensityPref, sidebarMode: 'advanced' | 'simple' | null): Density {
  if (pref !== 'auto') return pref
  return sidebarMode === 'advanced' ? 'compact' : 'comfortable'
}

export function readDensity(): Density {
  const pref = readDensityPref()
  if (pref !== 'auto') return pref
  // Sidebar mode key lives in localStorage ('dev' = Advanced); read directly
  // to avoid importing Sidebar (which pulls the React tree).
  let mode: 'advanced' | 'simple' | null = null
  try {
    const raw = localStorage.getItem('shannon-sidebar-mode')
    mode = raw === 'dev' ? 'advanced' : raw === 'basic' ? 'simple' : null
  } catch { /* noop */ }
  return resolveDensity(pref, mode)
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
