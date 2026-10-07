import { useT } from '@/i18n'
import ConnectionsSettings from '@/components/settings/ConnectionsSettings'
import RemotesSettings from '@/components/settings/RemotesSettings'
import NetworkSettings from '@/components/settings/NetworkSettings'

// IA redesign 2026-10 (ADVERSARIAL-REVIEW §2): 连接 absorbs 远程执行 and
// 网络 — one mental model ("where does the agent run, who does it talk
// to"), one credentials story. Order follows the design mockup: engine/
// gateway → remote targets → network. Each group heading reuses an existing
// title key; RemotesSettings renders its own h2 (settings.remotes.title), so
// it needs no wrapper heading. The old /settings/network and /settings/remotes
// deep links redirect here (App.tsx).
export default function ConnectionsPane() {
  const t = useT()
  return (
    <div className="space-y-xl">
      <section className="space-y-md" aria-labelledby="connections-pane-gateway-heading">
        <h2 id="connections-pane-gateway-heading" className="text-headline-sm font-medium">
          {t('settings.connections.title')}
        </h2>
        <ConnectionsSettings />
      </section>
      <section aria-label={t('settings.remotes.title')}>
        <RemotesSettings />
      </section>
      <section className="space-y-md" aria-labelledby="connections-pane-network-heading">
        <h2 id="connections-pane-network-heading" className="text-headline-sm font-medium">
          {t('nav.network')}
        </h2>
        <NetworkSettings />
      </section>
    </div>
  )
}
