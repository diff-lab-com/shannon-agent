import { useIntl } from 'react-intl'

/**
 * Settings → 会话 (Settings R3, T1 skeleton): the parity card lives in a
 * later commit of this batch — this placeholder keeps the route, the nav
 * entry, and the card chrome (title reuses the nav key so wording can't
 * drift) in place until then.
 */
export default function SessionSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return (
    <div className="pb-xl">
      <div className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1" data-testid="session-placeholder-card">
        <div className="flex items-center gap-md mb-md">
          <div className="p-sm bg-primary-container rounded-lg text-on-primary-container flex items-center justify-center">
            <span className="material-symbols-outlined" aria-hidden="true">forum</span>
          </div>
          <h3 className="font-headline-md text-headline-md font-bold text-on-surface">{t('nav.session')}</h3>
        </div>
        <p className="font-body-sm text-on-surface-variant">{t('settings.session.placeholder')}</p>
      </div>
    </div>
  )
}
