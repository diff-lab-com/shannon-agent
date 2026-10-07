import { useT } from '@/i18n'
import GeneralSettings from '@/components/settings/GeneralSettings'
import SessionSettings from '@/components/settings/SessionSettings'

// IA redesign 2026-10 (ADVERSARIAL-REVIEW §2): 会话 content was too thin to
// carry its own section — it is "general preferences" by mental model. The
// 通用 pane therefore stacks the two original components unchanged (the rail
// heading 通用 covers the whole pane; group headings reuse the existing
// `nav.*` keys), General on top, session defaults below. The old
// /settings/session deep link redirects here (App.tsx) — nothing is lost.
export default function GeneralPane() {
  const t = useT()
  return (
    <div className="space-y-xl">
      <section className="space-y-md" aria-labelledby="general-pane-general-heading">
        <h2 id="general-pane-general-heading" className="text-headline-sm font-medium">
          {t('nav.general')}
        </h2>
        <GeneralSettings />
      </section>
      <section className="space-y-md" aria-labelledby="general-pane-session-heading">
        <h2 id="general-pane-session-heading" className="text-headline-sm font-medium">
          {t('nav.session')}
        </h2>
        <SessionSettings />
      </section>
    </div>
  )
}
