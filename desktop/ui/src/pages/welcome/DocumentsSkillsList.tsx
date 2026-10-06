// Office Wave 1 A3' — honest documents-capabilities card. The three community
// skill repos this section used to offer (pandoc / python-docx / markdown
// beautify) are still unpublished, so install buttons are gone for good;
// instead the card surfaces the document skills that ARE built into the
// engine, gated on a real host probe: python3 present → list the slash
// commands; missing → say so and how to fix it. Renders nothing while the
// probe is in flight or failed — the card never claims anything it cannot
// verify (docs/research/2026-09-29-office-scenario-competitive-research.md
// §10 v2 A3').
import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { probeHostRuntime } from '@/lib/tauri-api'

const BUILTIN_DOCUMENT_SKILLS = ['/docx-report', '/xlsx-table', '/ppt-outline'] as const

export function DocumentsSkillsList() {
  const intl = useIntl()
  // null = probe still in flight or failed → stay silent rather than guess.
  const [python3Ready, setPython3Ready] = useState<boolean | null>(null)

  useEffect(() => {
    let cancelled = false
    probeHostRuntime()
      .then(probe => {
        if (!cancelled) setPython3Ready(Boolean(probe?.python3))
      })
      .catch(() => {
        if (!cancelled) setPython3Ready(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  if (python3Ready === null) return null

  return (
    <div
      className="mt-md p-md rounded-xl border border-outline-variant/50 bg-surface-container-low"
      data-testid="welcome-documents-skills"
    >
      <div className="flex items-center gap-xs mb-xs">
        <span className="material-symbols-outlined text-primary icon-md">extension</span>
        <span className="font-headline-md text-on-surface">
          {intl.formatMessage({ id: 'welcome.skills.title' })}
        </span>
      </div>
      {python3Ready ? (
        <>
          <p className="font-body-sm text-on-surface-variant mb-sm">
            {intl.formatMessage({ id: 'welcome.skills.builtinIntro' })}
          </p>
          <ul className="flex flex-wrap gap-sm" aria-label={intl.formatMessage({ id: 'welcome.skills.title' })}>
            {BUILTIN_DOCUMENT_SKILLS.map(cmd => (
              <li
                key={cmd}
                className="px-sm py-xs rounded-lg bg-surface-container-lowest border border-outline-variant/30 font-mono text-body-sm text-on-surface"
              >
                {cmd}
              </li>
            ))}
          </ul>
        </>
      ) : (
        <div className="flex items-start gap-sm">
          <span className="material-symbols-outlined text-on-surface-variant icon-md mt-[2px]" aria-hidden="true">
            info
          </span>
          <div>
            <div className="font-label-md text-on-surface">
              {intl.formatMessage({ id: 'welcome.skills.hostMissing.title' })}
            </div>
            <div className="font-body-sm text-on-surface-variant mt-[2px]">
              {intl.formatMessage({ id: 'welcome.skills.hostMissing.desc' })}
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
