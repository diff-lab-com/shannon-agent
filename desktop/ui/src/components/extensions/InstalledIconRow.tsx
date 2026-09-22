// Batch E3 (2026-09-20 delta analysis): the marketplace hub's 已安装 icon
// row — installed extensions surface as compact icon chips at the top of the
// browse page (the ZCode 市场首屏 pattern), one click from the full 已安装
// inventory. Hidden entirely while nothing is installed.
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { listInstalledAddons } from '@/lib/tauri-api'
import type { AddonKind, InstalledAddonSummary } from '@/types'
import { cn } from '@/lib/utils'

const MAX_ICONS = 12

const KIND_ICONS: Record<AddonKind, string> = {
  mcp: 'cloud',
  skill: 'extension',
  agent: 'smart_toy',
  data_source: 'database',
  plugin: 'workspaces',
}

export default function InstalledIconRow() {
  const intl = useIntl()
  const navigate = useNavigate()
  const [addons, setAddons] = useState<InstalledAddonSummary[]>([])

  useEffect(() => {
    let cancelled = false
    const load = () => {
      listInstalledAddons()
        .then(rows => { if (!cancelled) setAddons(rows) })
        .catch(() => { /* the row is opportunistic — hide on failure */ })
    }
    load()
    // Same refresh channel the Installed tab uses.
    window.addEventListener('shannon:extension-installed', load)
    return () => {
      cancelled = true
      window.removeEventListener('shannon:extension-installed', load)
    }
  }, [])

  if (addons.length === 0) return null
  const visible = addons.slice(0, MAX_ICONS)
  const overflow = addons.length - visible.length

  return (
    <div className="flex items-center gap-sm min-w-0" data-testid="installed-icon-row">
      <span className="font-label-xs uppercase tracking-wider text-on-surface-variant/70 shrink-0">
        {intl.formatMessage({ id: 'extensions.market.installedRow' })}
      </span>
      <div className="flex items-center gap-xs flex-wrap min-w-0">
        {visible.map(a => (
          <button
            key={a.id}
            type="button"
            onClick={() => navigate('/extensions/installed')}
            title={`${a.name} · ${a.kind}`}
            aria-label={a.name}
            className={cn(
              'w-9 h-9 rounded-lg flex items-center justify-center shrink-0 border transition-colors cursor-pointer',
              a.enabled
                ? 'bg-primary/10 border-primary/20 text-primary hover:bg-primary/20'
                : 'bg-surface-container-low border-outline-variant/30 text-on-surface-variant/60 hover:bg-surface-container',
            )}
          >
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">{KIND_ICONS[a.kind]}</span>
          </button>
        ))}
        {overflow > 0 && (
          <button
            type="button"
            onClick={() => navigate('/extensions/installed')}
            className="font-label-xs text-primary hover:underline cursor-pointer shrink-0 px-xs"
          >
            +{overflow}
          </button>
        )}
      </div>
    </div>
  )
}
