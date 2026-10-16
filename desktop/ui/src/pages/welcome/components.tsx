// Stepper + Card primitives shared across the four Welcome steps.
//
// Extracted from Welcome.tsx (T3.1) so the page orchestrator can stay focused
// on state + handlers. Kept in this file rather than `ui/components/` because
// they are scoped to the Welcome flow only — other pages have their own
// card variants.
import { useIntl } from 'react-intl'

export function Stepper({ step, labels }: { step: number; labels: string[] }) {
  const intl = useIntl()
  // Two-step onboarding (UI audit §3.1): callers pass the trimmed label list.
  // Round 6 §P3-1: removed the unused 4-key STEP_LABEL_KEYS default — every
  // Welcome step entrypoint now supplies its own label list.
  const labelKeys = labels
  const stepLabel = intl.formatMessage({ id: labelKeys[step] })
  // G4a (design 01:394-398 步骤指示): the PILL form — the current step rides
  // a violet-dim capsule with a violet hairline (第 1 步 · 共 2 步 —— 标签),
  // remaining steps stay as quiet muted text beside it. The container's
  // aria-label keeps announcing the full "Step x of y: label" sentence.
  return (
    <div
      className="flex items-center justify-center gap-sm flex-wrap mb-xl"
      aria-label={intl.formatMessage(
        { id: 'welcome.stepper.step' },
        { current: step + 1, total: labelKeys.length, label: stepLabel },
      )}
    >
      <span
        data-testid="welcome-stepper-pill"
        className="inline-flex items-center gap-xs rounded-full border border-primary/40 bg-primary-container text-on-primary-container px-md py-xs font-label-sm font-semibold whitespace-nowrap"
      >
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">flag</span>
        {intl.formatMessage(
          { id: 'welcome.stepper.step' },
          { current: step + 1, total: labelKeys.length, label: stepLabel },
        )}
      </span>
      {labelKeys.slice(step + 1).map(key => (
        <span key={key} className="font-label-sm text-on-surface-variant whitespace-nowrap">
          {intl.formatMessage({ id: key })}
        </span>
      ))}
    </div>
  )
}

export function WelcomeCard({ title, subtitle, footer, aurora, children }: {
  title: string
  subtitle: string
  footer: React.ReactNode
  /** W4 视觉收敛 (design 01b done-card `.glasscard aurora-line`): the flow's
   *  completion card carries the aurora top edge. Opt-in — the design only
   *  draws it on the done state (01b), not on the intermediate step cards. */
  aurora?: boolean
  children: React.ReactNode
}) {
  return (
    <section
      className={
        'bg-surface-container-lowest border border-outline-variant/30 rounded-2xl p-xl shadow-e1' +
        (aurora ? ' aurora-line' : '')
      }
    >
      <h1 className="font-headline-lg text-on-surface mb-xs">{title}</h1>
      <p className="font-body-md text-on-surface-variant mb-xl">{subtitle}</p>
      {children}
      <div className="flex justify-between items-center mt-xl">{footer}</div>
    </section>
  )
}