// DiagBanner — surfaces a diagnostic-fetch error or timeout. Pure: returns
// null when there is nothing to report.

interface DiagBannerProps {
  t: (id: string, values?: Record<string, string | number | boolean>) => string
  diagError: string | null
  diagTimedOut: boolean
}

export default function DiagBanner({ t, diagError, diagTimedOut }: DiagBannerProps) {
  if (!diagError && !diagTimedOut) return null
  return (
    <div
      className="bg-error/10 border border-error/30 rounded-lg p-sm font-label-sm text-error flex items-start gap-sm"
      role="status"
    >
      <span className="material-symbols-outlined icon-sm mt-0.5">
        warning
      </span>
      <span className="flex-1">
        {diagError
          ? `${t('editor.diagnosticsFailed', { error: diagError })}`
          : diagTimedOut
            ? t('editor.diagnosticsTimedOut')
            : null}
      </span>
    </div>
  )
}