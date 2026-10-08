import { useNavigate } from 'react-router-dom'
import { useT } from '@/i18n'
import { Button } from '@/components/ui/button'
import PermissionsSettings from '@/components/settings/PermissionsSettings'

// IA redesign 2026-10 (ADVERSARIAL-REVIEW §2): the design places the default
// execution tier (审批模式) inside 权限与安全. Moving the control out of
// GeneralSettings would break a group of tests and split the composer's
// shared `approval_mode` surface, so instead this pane opens with a guidance
// card: it names where the tier lives and jumps straight there. Section
// semantics are bridged by the card; the control itself stays on General.
export default function PermissionsPane() {
  const t = useT()
  const navigate = useNavigate()
  return (
    <div className="space-y-lg">
      <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1">
        <div className="flex items-start gap-md">
          <span className="material-symbols-outlined text-primary" aria-hidden="true">info</span>
          <div className="flex-1 space-y-sm">
            <h2 className="font-headline-sm text-headline-sm">
              {t('settings.permissions.defaultTierCard.title')}
            </h2>
            <p className="text-body-sm text-on-surface-variant max-w-prose">
              {t('settings.permissions.defaultTierCard.body')}
            </p>
            <Button
              variant="outline"
              size="sm"
              onClick={() => navigate('/settings/general')}
              data-testid="permissions-to-general-link"
            >
              {t('settings.permissions.defaultTierCard.action')}
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">arrow_forward</span>
            </Button>
          </div>
        </div>
      </section>
      <PermissionsSettings />
    </div>
  )
}
