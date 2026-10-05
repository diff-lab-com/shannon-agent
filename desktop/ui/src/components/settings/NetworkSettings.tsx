import { useEffect, useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { useCatalog } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { Button } from '@/components/ui/button'
import EffectBadge from './EffectBadge'

/**
 * Settings → 网络 (Settings R3, T4 / B1): the corporate-network trio.
 *
 * Three cards — HTTP proxy, NO_PROXY exceptions, custom CA certificate —
 * persisted via three `configure` calls (one per key, all-or-error per key,
 * like every other settings surface) and taking effect on the NEXT app
 * launch: the backend injects the standard proxy / CA env vars once, before
 * the tauri Builder starts, so the gateway sidecar, MCP stdio children and
 * the engine HTTP clients all inherit them. Every card therefore carries an
 * `EffectBadge kind="restart-app"`.
 *
 * R1 semantics (help copy): leaving the proxy empty keeps the implicit
 * "read proxy from the environment" fallback — Shannon never forces direct
 * connections just because the field is blank.
 */

const INPUT_CLASS =
  'bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30'

/** The proxy must be an http(s) URL — the env-injection path only speaks
 *  HTTP CONNECT proxies (socks:// would be silently ignored by reqwest's
 *  env handling in some clients). Empty passes: blank = clear (R1). */
export function validateProxyUrl(value: string): boolean {
  const trimmed = value.trim()
  if (!trimmed) return true
  return /^https?:\/\//i.test(trimmed)
}

export default function NetworkSettings() {
  const { config, refreshConfig } = useCatalog()
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [proxyUrl, setProxyUrl] = useState('')
  const [noProxy, setNoProxy] = useState('')
  const [caPath, setCaPath] = useState('')
  // Inline proxy-scheme error (set on blur, re-checked on save).
  const [proxyError, setProxyError] = useState(false)
  const [saving, setSaving] = useState(false)
  // The catalog config snapshot may arrive after first render (and again on
  // refreshConfig): hydrate the fields from it until the user edits each
  // one, so a slow load or a background refresh never clobbers typing.
  const touched = useRef({ proxy: false, noProxy: false, ca: false })

  useEffect(() => {
    if (!config) return
    if (!touched.current.proxy) setProxyUrl(config.network_proxy_url ?? '')
    if (!touched.current.noProxy) setNoProxy(config.network_no_proxy ?? '')
    if (!touched.current.ca) setCaPath(config.network_ca_cert_path ?? '')
  }, [config])

  const proxyInvalid = !validateProxyUrl(proxyUrl)
  const showProxyError = proxyError || (proxyUrl.trim() !== '' && proxyInvalid)

  const handleSave = async () => {
    if (proxyInvalid) {
      setProxyError(true)
      return
    }
    setSaving(true)
    try {
      // Three keys, three writes — the backend validates each (proxy
      // scheme, CA `~`-expansion + file existence) and persists + emits
      // CONFIG_UPDATED per key, mirroring every other configure arm.
      await api.configure({ key: 'network.proxy_url', value: proxyUrl.trim() })
      await api.configure({ key: 'network.no_proxy', value: noProxy.trim() })
      await api.configure({ key: 'network.ca_cert_path', value: caPath.trim() })
      await refreshConfig()
      toast.success(t('settings.network.saved'))
    } catch (e) {
      toastError(t('settings.network.saveFailed'), e)
    }
    setSaving(false)
  }

  return (
    <div className="pb-xl space-y-lg">
      {/* ① HTTP proxy */}
      <section
        className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2"
        data-testid="network-proxy-card"
      >
        <div className="flex items-center gap-md mb-xs">
          <span className="material-symbols-outlined text-primary" style={{ fontVariationSettings: "'FILL' 1" }} aria-hidden="true">lan</span>
          <h3 className="font-headline-md text-headline-md">{t('settings.network.proxy')}</h3>
          <span className="flex-1" />
          <EffectBadge kind="restart-app" />
        </div>
        <p className="font-body-sm text-on-surface-variant mb-md">{t('settings.network.proxyHelp')}</p>
        <input
          type="text"
          value={proxyUrl}
          onChange={e => {
            touched.current.proxy = true
            setProxyUrl(e.target.value)
            // A fix typed after a failed blur check clears the error
            // immediately — no need to blur again.
            if (validateProxyUrl(e.target.value)) setProxyError(false)
          }}
          onBlur={() => setProxyError(proxyInvalid)}
          placeholder="http://127.0.0.1:7890"
          aria-label={t('settings.network.proxy')}
          aria-invalid={showProxyError}
          data-testid="network-proxy-input"
          className={`${INPUT_CLASS} w-full max-w-md ${showProxyError ? 'border-error ring-1 ring-error/40' : ''}`}
        />
        {showProxyError && (
          <p
            className="font-body-sm text-error mt-xs"
            role="alert"
            data-testid="network-proxy-error"
          >
            {t('settings.network.invalidProxy')}
          </p>
        )}
      </section>

      {/* ② NO_PROXY exceptions */}
      <section
        className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2"
        data-testid="network-no-proxy-card"
      >
        <div className="flex items-center gap-md mb-xs">
          <span className="material-symbols-outlined text-primary" style={{ fontVariationSettings: "'FILL' 1" }} aria-hidden="true">block</span>
          <h3 className="font-headline-md text-headline-md">{t('settings.network.noProxy')}</h3>
          <span className="flex-1" />
          <EffectBadge kind="restart-app" />
        </div>
        <p className="font-body-sm text-on-surface-variant mb-md">{t('settings.network.noProxyHelp')}</p>
        <input
          type="text"
          value={noProxy}
          onChange={e => {
            touched.current.noProxy = true
            setNoProxy(e.target.value)
          }}
          placeholder="localhost,127.0.0.1,::1,.example.com"
          aria-label={t('settings.network.noProxy')}
          data-testid="network-no-proxy-input"
          className={`${INPUT_CLASS} w-full max-w-md`}
        />
      </section>

      {/* ③ Custom CA certificate */}
      <section
        className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2"
        data-testid="network-ca-card"
      >
        <div className="flex items-center gap-md mb-xs">
          <span className="material-symbols-outlined text-primary" style={{ fontVariationSettings: "'FILL' 1" }} aria-hidden="true">verified_user</span>
          <h3 className="font-headline-md text-headline-md">{t('settings.network.ca')}</h3>
          <span className="flex-1" />
          <EffectBadge kind="restart-app" />
        </div>
        <p className="font-body-sm text-on-surface-variant mb-md">{t('settings.network.caHelp')}</p>
        <input
          type="text"
          value={caPath}
          onChange={e => {
            touched.current.ca = true
            setCaPath(e.target.value)
          }}
          placeholder="~/certs/root-ca.pem"
          aria-label={t('settings.network.ca')}
          data-testid="network-ca-input"
          className={`${INPUT_CLASS} w-full max-w-md`}
        />
      </section>

      {/* Unified save — the three writes land together so the card can give
          one verdict (and one toast) instead of three. */}
      <div className="flex items-center gap-md">
        <Button
          onClick={handleSave}
          disabled={saving}
          data-testid="network-save"
          className="px-lg py-sm rounded-lg font-label-md cursor-pointer"
        >
          {t('settings.network.save')}
        </Button>
      </div>
    </div>
  )
}
