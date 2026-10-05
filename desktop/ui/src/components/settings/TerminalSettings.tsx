import { useCallback, useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import EffectBadge from '@/components/settings/EffectBadge'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import type { TerminalSettings as TerminalSettingsDto } from '@/types'

/** Task 12: placeholder mirroring the built-in monospace fallback stack. */
const FONT_FAMILY_PLACEHOLDER = "'JetBrains Mono', 'Fira Code', monospace"

/**
 * P3-1 — Advanced-settings card for the integrated terminal's persisted
 * preferences (`[terminal]` in `~/.shannon/config.toml`, served by the
 * `terminal_get_settings` / `terminal_set_settings` commands).
 *
 * Contract notes:
 *  - The backend clamps every numeric knob (fontSize 8–32, scrollback
 *    0–100000, drawerHeight 120–1200), truncates the font stack to 200
 *    chars and trims/blanks the shell + font family. The frontend sends
 *    the raw values and then renders the EFFECTIVE values from the
 *    set-response, so what's on screen is what's on disk.
 *  - Changes only reach terminals opened afterwards — live xterm
 *    instances are never re-geometried behind the user's back.
 *  - Load failure is NOT an editable empty form: every field disables,
 *    Save stays disabled until a load SUCCEEDS, and an error banner with
 *    retry takes the card. Saving `Number('') = 0` for untouched numerics
 *    would let the backend clamp them onto the minimums and silently
 *    clobber the stored config — that must never be one click away.
 */
export function TerminalSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  // Inputs stay strings so the user can type freely (mid-edit "",
  // partial numbers); parsed raw on save — the backend clamps.
  const [shell, setShell] = useState('')
  const [fontSize, setFontSize] = useState('')
  const [scrollback, setScrollback] = useState('')
  const [drawerHeight, setDrawerHeight] = useState('')
  const [screenReaderMode, setScreenReaderMode] = useState(false)
  // Task 12: login-shell inheritance (switch) + font family override
  // (kept as a string so the user can type freely; blank = unset).
  const [loginShell, setLoginShell] = useState(false)
  const [fontFamily, setFontFamily] = useState('')
  const [loading, setLoading] = useState(true)
  // True only after a SUCCESSFUL load — Save stays gated on it so a
  // failed load can never springboard a clobbering save from empty inputs.
  const [loaded, setLoaded] = useState(false)
  const [loadFailed, setLoadFailed] = useState(false)
  const [saving, setSaving] = useState(false)

  /** Render the effective values (used for both load and save responses). */
  const apply = useCallback((s: TerminalSettingsDto) => {
    setShell(s.shell ?? '')
    setFontSize(String(s.fontSize))
    setScrollback(String(s.scrollback))
    setDrawerHeight(String(s.drawerHeight))
    setScreenReaderMode(s.screenReaderMode)
    setLoginShell(s.loginShell)
    setFontFamily(s.fontFamily ?? '')
  }, [])

  const load = useCallback(() => {
    setLoading(true)
    setLoadFailed(false)
    api.terminalGetSettings()
      .then((s) => { apply(s); setLoaded(true) })
      .catch((e) => {
        setLoadFailed(true)
        toastError(t('settings.terminal.loadFailed'), e)
      })
      .finally(() => { setLoading(false) })
    // eslint-disable-next-line react-hooks/exhaustive-deps -- one-shot loader + retry entry; `t` follows the intl provider
  }, [apply])

  useEffect(() => {
    load()
  }, [load])

  const handleSave = async () => {
    // Belt and braces: without a successful load the inputs hold nothing
    // meaningful — never let a save fire from them.
    if (!loaded) return
    setSaving(true)
    try {
      const effective = await api.terminalSetSettings({
        shell: shell.trim() === '' ? null : shell.trim(),
        fontSize: Number(fontSize),
        scrollback: Number(scrollback),
        drawerHeight: Number(drawerHeight),
        screenReaderMode,
        // Task 12: blank font family = built-in monospace stack (null on
        // the wire, skipped key on disk); login-shell rides as a bool.
        loginShell,
        fontFamily: fontFamily.trim() === '' ? null : fontFamily.trim(),
      })
      // Show what the backend actually stored (clamped into range), not
      // the raw input.
      apply(effective)
      toast.success(t('settings.terminal.saved'))
    } catch (e) {
      toastError(t('settings.terminal.saveFailed'), e)
    }
    setSaving(false)
  }

  // Every field is dead until a load succeeds (loading) and stays dead
  // after a failed one (loadFailed) — retry is the only way back.
  const fieldsDisabled = loading || loadFailed

  return (
    <div className="bg-surface-container-lowest p-lg rounded-xl shadow-e1 border border-outline-variant/30 lg:col-span-2 group hover:shadow-e2 transition-shadow" data-testid="terminal-settings-card">
      <div className="flex items-center gap-md mb-md">
        <div className="p-2 bg-primary-container rounded-lg text-on-primary-container flex items-center justify-center">
          <span className="material-symbols-outlined">terminal</span>
        </div>
        <h3 className="font-headline-md text-headline-md font-bold text-on-surface">{t('settings.terminal.title')}</h3>
      </div>
      <p className="text-on-surface-variant text-body-sm mb-lg">{t('settings.terminal.description')}</p>

      {loadFailed && (
        <div
          role="alert"
          data-testid="terminal-settings-load-error"
          className="flex flex-col md:flex-row md:items-center justify-between gap-sm p-sm mb-md rounded-lg bg-error/5 border border-error/20"
        >
          <p className="text-body-sm text-on-surface-variant">{t('settings.terminal.loadFailedDesc')}</p>
          <Button
            variant="ghost"
            className="flex items-center gap-xs text-link font-label-md text-label-md hover:underline cursor-pointer shrink-0"
            onClick={load}
            aria-label={t('settings.terminal.loadRetry')}
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">refresh</span>
            {t('settings.terminal.loadRetry')}
          </Button>
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
        <label className="flex flex-col gap-xs md:col-span-2">
          <span className="font-label-sm text-label-sm text-on-surface-variant">
            {t('settings.terminal.shell')}
          </span>
          <input
            type="text"
            value={shell}
            onChange={e => setShell(e.target.value)}
            disabled={fieldsDisabled}
            aria-label={t('settings.terminal.shell')}
            className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30 disabled:opacity-50"
          />
          <span className="font-label-sm text-label-sm text-on-surface-variant">
            {t('settings.terminal.shellHint')}
          </span>
        </label>

        {/* Task 12 — terminal font family (blank = built-in stack). */}
        <label className="flex flex-col gap-xs md:col-span-2">
          <span className="flex items-center gap-sm">
            <span className="font-label-sm text-label-sm text-on-surface-variant">
              {t('settings.terminal.fontFamily.label')}
            </span>
            <EffectBadge kind="new-session" />
          </span>
          <input
            type="text"
            value={fontFamily}
            onChange={e => setFontFamily(e.target.value)}
            disabled={fieldsDisabled}
            placeholder={FONT_FAMILY_PLACEHOLDER}
            aria-label={t('settings.terminal.fontFamily.label')}
            data-testid="terminal-font-family-input"
            className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30 disabled:opacity-50 placeholder:text-on-surface-variant/50"
          />
          <span className="font-label-sm text-label-sm text-on-surface-variant">
            {t('settings.terminal.fontFamily.desc')}
          </span>
        </label>

        <label className="flex flex-col gap-xs">
          <span className="font-label-sm text-label-sm text-on-surface-variant">
            {t('settings.terminal.fontSize')}
          </span>
          <input
            type="number"
            min={8}
            max={32}
            value={fontSize}
            onChange={e => setFontSize(e.target.value)}
            disabled={fieldsDisabled}
            aria-label={t('settings.terminal.fontSize')}
            className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30 disabled:opacity-50"
          />
        </label>

        <label className="flex flex-col gap-xs">
          <span className="font-label-sm text-label-sm text-on-surface-variant">
            {t('settings.terminal.scrollback')}
          </span>
          <input
            type="number"
            min={0}
            max={100000}
            value={scrollback}
            onChange={e => setScrollback(e.target.value)}
            disabled={fieldsDisabled}
            aria-label={t('settings.terminal.scrollback')}
            className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30 disabled:opacity-50"
          />
        </label>

        <label className="flex flex-col gap-xs">
          <span className="font-label-sm text-label-sm text-on-surface-variant">
            {t('settings.terminal.drawerHeight')}
          </span>
          <input
            type="number"
            min={120}
            max={1200}
            value={drawerHeight}
            onChange={e => setDrawerHeight(e.target.value)}
            disabled={fieldsDisabled}
            aria-label={t('settings.terminal.drawerHeight')}
            className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30 disabled:opacity-50"
          />
        </label>

        {/* Task 12 — inherit login shell environment (login shell spawn). */}
        <div className="flex items-center justify-between gap-md">
          <div>
            <div className="flex items-center gap-sm font-label-md text-label-md text-on-surface font-semibold mb-1">
              {t('settings.terminal.loginShell.label')}
              <EffectBadge kind="new-session" />
            </div>
            <div className="font-label-sm text-label-sm text-on-surface-variant leading-tight">
              {t('settings.terminal.loginShell.desc')}
            </div>
          </div>
          <Switch
            checked={loginShell}
            onCheckedChange={setLoginShell}
            disabled={fieldsDisabled}
            className="shrink-0"
            aria-label={t('settings.terminal.loginShell.label')}
            data-testid="terminal-login-shell-switch"
          />
        </div>

        <div className="flex items-center justify-between gap-md">
          <div>
            <div className="font-label-md text-label-md text-on-surface font-semibold mb-1">
              {t('settings.terminal.screenReaderMode')}
            </div>
            <div className="font-label-sm text-label-sm text-on-surface-variant leading-tight">
              {t('settings.terminal.screenReaderModeDesc')}
            </div>
          </div>
          <Switch
            checked={screenReaderMode}
            onCheckedChange={setScreenReaderMode}
            disabled={fieldsDisabled}
            className="shrink-0"
            aria-label={t('settings.terminal.screenReaderMode')}
          />
        </div>
      </div>

      <div className="flex justify-end mt-md">
        <Button
          className="px-xl py-md bg-primary text-on-primary rounded-lg font-label-md text-label-md font-bold hover:bg-primary/90 active:scale-[0.98] transition-all cursor-pointer disabled:opacity-50"
          onClick={() => void handleSave()}
          disabled={saving || loading || !loaded}
          aria-label={t('settings.terminal.saveAria')}
        >
          {saving ? t('settings.terminal.saving') : t('settings.terminal.save')}
        </Button>
      </div>
    </div>
  )
}
