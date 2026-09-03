// Stepper + Card primitives shared across the four Welcome steps.
//
// Extracted from Welcome.tsx (T3.1) so the page orchestrator can stay focused
// on state + handlers. Kept in this file rather than `ui/components/` because
// they are scoped to the Welcome flow only — other pages have their own
// card variants.
import { useIntl } from 'react-intl'
import { cn } from '@/lib/utils'
import { STEP_LABEL_KEYS } from './constants'

export function Stepper({ step }: { step: number }) {
  const intl = useIntl()
  const stepLabel = intl.formatMessage({ id: STEP_LABEL_KEYS[step] })
  // Dots on top, labels always visible underneath — an inline label after
  // the current dot (the old layout) shifted the whole row's geometry on
  // every step change and left the dots unaligned.
  return (
    <div
      className="flex items-start justify-center mb-xl"
      aria-label={intl.formatMessage(
        { id: 'welcome.stepper.step' },
        { current: step + 1, total: STEP_LABEL_KEYS.length, label: stepLabel },
      )}
    >
      {STEP_LABEL_KEYS.map((key, i) => (
        <div key={key} className="flex items-start">
          <div className="flex flex-col items-center gap-xs w-20">
            <div
              className={cn(
                'w-2 h-2 rounded-full mt-1.5',
                i <= step ? 'bg-primary' : 'bg-outline-variant',
              )}
            />
            <span
              className={cn(
                'font-label-sm text-center',
                i === step ? 'text-primary font-bold' : 'text-on-surface-variant',
              )}
            >
              {intl.formatMessage({ id: key })}
            </span>
          </div>
          {i < STEP_LABEL_KEYS.length - 1 && (
            <div className="w-8 h-px bg-outline-variant mt-1.5" aria-hidden="true" />
          )}
        </div>
      ))}
    </div>
  )
}

export function WelcomeCard({ title, subtitle, footer, children }: {
  title: string
  subtitle: string
  footer: React.ReactNode
  children: React.ReactNode
}) {
  return (
    <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-2xl p-xl shadow-sm">
      <h1 className="font-headline-lg text-on-surface mb-xs">{title}</h1>
      <p className="font-body-md text-on-surface-variant mb-xl">{subtitle}</p>
      {children}
      <div className="flex justify-between items-center mt-xl">{footer}</div>
    </section>
  )
}