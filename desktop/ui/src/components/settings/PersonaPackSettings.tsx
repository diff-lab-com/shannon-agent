import { useState } from 'react'
import { useIntl } from 'react-intl'
import { save as saveDialog, open as openDialog } from '@tauri-apps/plugin-dialog'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/loading-state'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import type {
  PersonaPackConflict,
  PersonaPackCounts,
  PersonaPackInclude,
} from '@/lib/tauri-api'
import { cn } from '@/lib/utils'

const CATEGORIES: { key: keyof PersonaPackInclude; labelKey: string }[] = [
  { key: 'skills', labelKey: 'settings.personaPack.cat.skills' },
  { key: 'commands', labelKey: 'settings.personaPack.cat.commands' },
  { key: 'memory', labelKey: 'settings.personaPack.cat.memory' },
  { key: 'routines', labelKey: 'settings.personaPack.cat.routines' },
  { key: 'profiles', labelKey: 'settings.personaPack.cat.profiles' },
  { key: 'persona', labelKey: 'settings.personaPack.cat.persona' },
]

/** Wire `PackCounts` field for a category (`memory` → `memories`). */
const COUNTS_KEY: Record<keyof PersonaPackInclude, keyof PersonaPackCounts> = {
  skills: 'skills',
  commands: 'commands',
  memory: 'memories',
  routines: 'routines',
  profiles: 'profiles',
  persona: 'persona',
}

const ALL_FALSE: PersonaPackInclude = {
  skills: false,
  commands: false,
  memory: false,
  routines: false,
  profiles: false,
  persona: false,
}

const CONFLICTS: { value: PersonaPackConflict; labelKey: string }[] = [
  { value: 'skip', labelKey: 'settings.personaPack.conflict.skip' },
  { value: 'overwrite', labelKey: 'settings.personaPack.conflict.overwrite' },
  { value: 'rename', labelKey: 'settings.personaPack.conflict.rename' },
]

export default function PersonaPackSettings() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  // ─── Export state ───
  const [include, setInclude] = useState<PersonaPackInclude>({ ...ALL_FALSE })
  const [exporting, setExporting] = useState(false)
  const [exportResult, setExportResult] = useState<api.PersonaPackExportResult | null>(null)

  // ─── Import state ───
  const [inspect, setInspect] = useState<api.PersonaPackInspectResult | null>(null)
  const [inspectPath, setInspectPath] = useState<string | null>(null)
  const [inspecting, setInspecting] = useState(false)
  const [conflict, setConflict] = useState<PersonaPackConflict>('skip')
  const [importing, setImporting] = useState(false)
  const [importReport, setImportReport] = useState<api.PersonaPackImportReport | null>(null)

  const toggle = (key: keyof PersonaPackInclude) =>
    setInclude(prev => ({ ...prev, [key]: !prev[key] }))

  const handleExport = async () => {
    const defaultName = `shannon-pack-${new Date().toISOString().slice(0, 10)}.tar.gz`
    let target: string | null = null
    try {
      target = await saveDialog({
        defaultPath: defaultName,
        filters: [{ name: 'Shannon pack (tar.gz)', extensions: ['tar.gz', 'gz'] }],
      })
    } catch (e) {
      toastError(String(e))
      return
    }
    if (!target) return // user cancelled
    setExporting(true)
    setExportResult(null)
    try {
      const result = await api.personaPackExport(target, include)
      setExportResult(result)
      toast.success(t('settings.personaPack.export.doneToast', { path: result.path }))
    } catch (e) {
      toastError(String(e))
    }
    setExporting(false)
  }

  const handlePickPack = async () => {
    let target: string | null = null
    try {
      target = await openDialog({
        multiple: false,
        filters: [{ name: 'Shannon pack (tar.gz)', extensions: ['tar.gz', 'gz'] }],
      })
    } catch (e) {
      toastError(String(e))
      return
    }
    if (!target || typeof target !== 'string') return // user cancelled
    setInspecting(true)
    setInspect(null)
    setImportReport(null)
    setInspectPath(null)
    try {
      const info = await api.personaPackInspect(target)
      setInspect(info)
      setInspectPath(target)
    } catch (e) {
      toastError(String(e))
    }
    setInspecting(false)
  }

  const handleImport = async () => {
    if (!inspectPath) return
    setImporting(true)
    try {
      const report = await api.personaPackImport(inspectPath, conflict, include)
      setImportReport(report)
      toast.success(t('settings.personaPack.import.doneToast'))
    } catch (e) {
      toastError(String(e))
    }
    setImporting(false)
  }

  const toastError = (message: string) => {
    toast.error(t('settings.personaPack.failedToast'), { description: message })
  }

  return (
    <section
      className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm"
      data-testid="persona-pack-section"
    >
      <div className="flex items-center gap-md mb-xs">
        <span
          className="material-symbols-outlined text-primary"
          style={{ fontVariationSettings: "'FILL' 1" }}
          aria-hidden="true"
        >
          folder_zip
        </span>
        <h3 className="font-headline-md text-headline-md">{t('settings.personaPack.title')}</h3>
      </div>
      <p className="font-body-sm text-on-surface-variant mb-xl">{t('settings.personaPack.desc')}</p>

      {/* ─── Export ─── */}
      <div className="mb-lg">
        <h4 className="font-label-lg text-on-surface mb-sm">{t('settings.personaPack.export.title')}</h4>
        <fieldset className="mb-md">
          <legend className="font-label-sm text-on-surface-variant mb-xs">
            {t('settings.personaPack.export.includeLegend')}
          </legend>
          <div className="grid grid-cols-2 gap-sm">
            {CATEGORIES.map(cat => (
              <label
                key={cat.key}
                htmlFor={`persona-pack-include-${cat.key}`}
                className="flex items-center gap-sm font-label-md text-on-surface cursor-pointer select-none"
              >
                <input
                  type="checkbox"
                  id={`persona-pack-include-${cat.key}`}
                  checked={include[cat.key]}
                  onChange={() => toggle(cat.key)}
                  className="accent-primary"
                  data-testid={`persona-pack-include-${cat.key}`}
                />
                {t(cat.labelKey)}
              </label>
            ))}
          </div>
        </fieldset>
        <Button
          onClick={handleExport}
          disabled={exporting}
          data-testid="persona-pack-export"
          aria-label={t('settings.personaPack.export.buttonAria')}
          className="px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all bg-primary text-on-primary hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
        >
          {exporting ? (
            <span className="flex items-center gap-sm">
              <Spinner className="text-on-primary text-[16px]" />
              {t('settings.personaPack.export.working')}
            </span>
          ) : (
            t('settings.personaPack.export.button')
          )}
        </Button>

        {exportResult && (
          <div
            className="mt-md p-md rounded-lg bg-surface-container border border-outline-variant/40"
            data-testid="persona-pack-export-result"
            role="status"
          >
            <p className="font-label-md text-on-surface mb-xs">
              {t('settings.personaPack.export.doneTitle')}
            </p>
            <p className="font-body-sm text-on-surface-variant break-all mb-sm">{exportResult.path}</p>
            <ul className="flex flex-wrap gap-sm mb-sm">
              {CATEGORIES.map(cat => (
                <li
                  key={cat.key}
                  className="font-label-sm bg-surface-container-high text-on-surface rounded px-sm py-0.5"
                  data-testid={`persona-pack-export-count-${cat.key}`}
                >
                  {t(cat.labelKey)}: {exportResult.counts[COUNTS_KEY[cat.key]]}
                </li>
              ))}
            </ul>
            {exportResult.stripped > 0 ? (
              <p
                className="font-body-sm text-warning"
                data-testid="persona-pack-stripped-note"
              >
                {t('settings.personaPack.export.stripped', { count: exportResult.stripped })}
              </p>
            ) : (
              <p className="font-body-sm text-on-surface-variant">
                {t('settings.personaPack.export.noSecrets')}
              </p>
            )}
          </div>
        )}
      </div>

      {/* ─── Import ─── */}
      <div>
        <h4 className="font-label-lg text-on-surface mb-sm">{t('settings.personaPack.import.title')}</h4>
        <Button
          variant="outline"
          onClick={handlePickPack}
          disabled={inspecting}
          data-testid="persona-pack-pick"
          className="px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all bg-surface-container-low hover:bg-surface-container-high border border-outline-variant/50 text-on-surface focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
        >
          {inspecting ? t('settings.personaPack.import.inspecting') : t('settings.personaPack.import.pick')}
        </Button>

        {inspect && (
          <div className="mt-md" data-testid="persona-pack-preview">
            <table className="w-full font-body-sm mb-sm" role="table" aria-label={t('settings.personaPack.previewAria')}>
              <caption className="text-left font-label-md text-on-surface mb-xs">
                {t('settings.personaPack.import.previewTitle', {
                  generator: inspect.generator,
                  version: inspect.version,
                })}
              </caption>
              <thead>
                <tr className="text-left text-on-surface-variant border-b border-outline-variant/40">
                  <th scope="col" className="py-xs font-medium">{t('settings.personaPack.table.category')}</th>
                  <th scope="col" className="py-xs font-medium">{t('settings.personaPack.table.count')}</th>
                </tr>
              </thead>
              <tbody>
                {CATEGORIES.map(cat => (
                  <tr key={cat.key} className="border-b border-outline-variant/20">
                    <td className="py-xs text-on-surface">{t(cat.labelKey)}</td>
                    <td className="py-xs text-on-surface" data-testid={`persona-pack-preview-${cat.key}`}>
                      {inspect.counts[COUNTS_KEY[cat.key]]}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>

            <fieldset className="mb-md">
              <legend className="font-label-sm text-on-surface-variant mb-xs">
                {t('settings.personaPack.import.conflictLegend')}
              </legend>
              <div className="flex flex-col gap-xs">
                {CONFLICTS.map(c => (
                  <label
                    key={c.value}
                    htmlFor={`persona-pack-conflict-${c.value}`}
                    className="flex items-center gap-sm font-label-md text-on-surface cursor-pointer select-none"
                  >
                    <input
                      type="radio"
                      id={`persona-pack-conflict-${c.value}`}
                      name="persona-pack-conflict"
                      checked={conflict === c.value}
                      onChange={() => setConflict(c.value)}
                      className="accent-primary"
                      data-testid={`persona-pack-conflict-${c.value}`}
                    />
                    {t(c.labelKey)}
                  </label>
                ))}
              </div>
            </fieldset>

            <Button
              onClick={handleImport}
              disabled={importing}
              data-testid="persona-pack-import"
              className={cn(
                'px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all bg-primary text-on-primary',
                'hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed',
                'focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
              )}
            >
              {importing ? t('settings.personaPack.import.working') : t('settings.personaPack.import.button')}
            </Button>
          </div>
        )}

        {importReport && (
          <div
            className="mt-md p-md rounded-lg bg-surface-container border border-outline-variant/40"
            data-testid="persona-pack-import-result"
            role="status"
          >
            <p className="font-label-md text-on-surface mb-xs">{t('settings.personaPack.import.doneTitle')}</p>
            <ul className="flex flex-wrap gap-sm mb-sm">
              {CATEGORIES.map(cat => (
                <li
                  key={cat.key}
                  className="font-label-sm bg-surface-container-high text-on-surface rounded px-sm py-0.5"
                  data-testid={`persona-pack-imported-${cat.key}`}
                >
                  {t(cat.labelKey)}: {importReport.imported[COUNTS_KEY[cat.key]]} / {importReport.skipped[COUNTS_KEY[cat.key]]}
                </li>
              ))}
            </ul>
            {importReport.failed.length > 0 ? (
              <ul className="font-body-sm text-error space-y-xs" data-testid="persona-pack-failures">
                {importReport.failed.map(f => (
                  <li key={`${f.item}-${f.error}`}>
                    {f.item}: {f.error}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="font-body-sm text-on-surface-variant">{t('settings.personaPack.import.noFailures')}</p>
            )}
          </div>
        )}
      </div>
    </section>
  )
}
