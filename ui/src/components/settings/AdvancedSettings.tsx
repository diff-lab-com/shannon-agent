import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'

export default function AdvancedSettings() {
  const { refreshConfig, config } = useApp()
  const [memoryEnabled, setMemoryEnabled] = useState(config?.memory_enabled ?? true)
  const [telemetryEnabled, setTelemetryEnabled] = useState(config?.telemetry_enabled ?? false)
  const [encryptionEnabled, setEncryptionEnabled] = useState(config?.encryption_enabled ?? true)
  const [debugConsole, setDebugConsole] = useState(config?.debug_console ?? false)
  const [clearing, setClearing] = useState(false)
  const [resetting, setResetting] = useState(false)
  const [showLogs, setShowLogs] = useState(false)
  const [showApiKeys, setShowApiKeys] = useState(false)
  const [showResetConfirm, setShowResetConfirm] = useState(false)
  const [showClearConfirm, setShowClearConfirm] = useState(false)

  const handleToggle = async (key: string, value: boolean, setter: (v: boolean) => void) => {
    setter(value)
    try {
      await api.configure({ key, value: String(value) })
      await refreshConfig()
      toast.success(`${key.replace(/_/g, ' ')} ${value ? 'enabled' : 'disabled'}`)
    } catch (e) { console.warn("AdvancedSettings error:", e); toast.error('Failed to update setting') }
  }

  const handleClearCache = async () => {
    setClearing(true)
    try { await api.configure({ key: 'clear_cache', value: 'true' }); toast.success('Cache cleared') } catch (e) { console.warn("AdvancedSettings error:", e); toast.error('Failed to clear cache') }
    setClearing(false)
  }

  const handleFactoryReset = async () => {
    setResetting(true)
    try { await api.configure({ key: 'factory_reset', value: 'true' }); toast.success('Factory reset complete') } catch (e) { console.warn("AdvancedSettings error:", e); toast.error('Factory reset failed') }
    setResetting(false)
    setShowResetConfirm(false)
  }

  return (
    <div className="pb-xl">
      <div className="mb-xl">
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-sm">Advanced Settings</h2>
        <p className="text-on-surface-variant font-body-md">Configure underlying engine parameters and data sovereignty protocols.</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-gutter">
        {/* Memory Management */}
        <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 group hover:shadow-md transition-shadow">
          <div className="flex items-center gap-md mb-md">
            <div className="p-2 bg-primary/10 rounded-lg text-primary flex items-center justify-center">
              <span className="material-symbols-outlined">memory</span>
            </div>
            <h3 className="font-headline-md text-[24px] font-bold text-on-surface">Memory Management</h3>
          </div>
          <p className="text-on-surface-variant text-body-sm mb-lg">Manage how the AI persists context and session artifacts over time.</p>
          <div className="space-y-md">
            <div className="flex items-center justify-between py-sm gap-md">
              <div>
                <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">Long-term Memory</div>
                <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">Allow agent to reference past conversations.</div>
              </div>
              <Switch checked={memoryEnabled} onCheckedChange={v => handleToggle('memory_enabled', v, setMemoryEnabled)} className="shrink-0" />
            </div>
            <Button
              className="w-full py-md border border-outline-variant/50 rounded-xl text-on-surface font-label-md font-bold text-[14px] hover:bg-surface-container-low transition-colors active:scale-[0.99] cursor-pointer"
              onClick={() => setShowClearConfirm(true)}
              disabled={clearing}
            >
              {clearing ? <span className="material-symbols-outlined animate-spin mr-sm text-[18px]">progress_activity</span> : null}
              {clearing ? 'Clearing...' : 'Clear Session Cache'}
            </Button>
          </div>
        </div>

        {/* Data Privacy */}
        <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 group hover:shadow-md transition-shadow">
          <div className="flex items-center gap-md mb-md">
            <div className="p-2 bg-secondary/10 rounded-lg text-secondary flex items-center justify-center">
              <span className="material-symbols-outlined" style={{fontVariationSettings: "'FILL' 1"}}>security</span>
            </div>
            <h3 className="font-headline-md text-[24px] font-bold text-on-surface">Data Privacy</h3>
          </div>
          <p className="text-on-surface-variant text-body-sm mb-lg">Control your cryptographic signatures and usage telemetry protocols.</p>
          <div className="space-y-lg mt-sm">
            <div className="flex items-center justify-between gap-md">
              <div>
                <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">Anonymous Usage Reporting</div>
                <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">Share diagnostic data to improve models.</div>
              </div>
              <Switch checked={telemetryEnabled} onCheckedChange={v => handleToggle('telemetry', v, setTelemetryEnabled)} className="shrink-0" />
            </div>
            <div className="flex items-center justify-between gap-md">
              <div>
                <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">Local Data Encryption</div>
                <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">Encrypt database with AES-256 standard.</div>
              </div>
              <Switch checked={encryptionEnabled} onCheckedChange={v => handleToggle('encryption', v, setEncryptionEnabled)} className="shrink-0" />
            </div>
          </div>
        </div>

        {/* Developer Options */}
        <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 lg:col-span-2 group hover:shadow-md transition-shadow">
          <div className="flex items-center gap-md mb-md">
            <div className="p-2 bg-tertiary/10 rounded-lg text-tertiary flex items-center justify-center">
              <span className="material-symbols-outlined">terminal</span>
            </div>
            <h3 className="font-headline-md text-[24px] font-bold text-on-surface">Developer Options</h3>
          </div>
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-lg">
            <div className="flex-1">
              <p className="text-on-surface-variant text-body-sm mb-md">Advanced tools for debugging agent behaviors and observing raw kernel output.</p>
              <div className="flex items-center gap-md">
                <Button variant="ghost" className="flex items-center gap-xs text-primary font-label-md text-[14px] hover:underline cursor-pointer" onClick={() => setShowLogs(true)}>
                  <span className="material-symbols-outlined text-[16px]">description</span>
                  View System Logs
                </Button>
                <span className="text-outline-variant">|</span>
                <Button variant="ghost" className="flex items-center gap-xs text-primary font-label-md text-[14px] hover:underline cursor-pointer" onClick={() => setShowApiKeys(true)}>
                  <span className="material-symbols-outlined text-[16px]">api</span>
                  Manage API Keys
                </Button>
              </div>
            </div>
            <div className="flex items-center gap-md bg-surface-container-low p-md rounded-xl border border-outline-variant/20 shrink-0">
              <span className="font-label-md text-[14px] text-on-surface">Enable Debug Console</span>
              <Switch checked={debugConsole} onCheckedChange={v => handleToggle('debug_console', v, setDebugConsole)} />
            </div>
          </div>
        </div>

        {/* Critical System Reset */}
        <div className="lg:col-span-2 border-2 border-error/20 bg-error/5 p-lg rounded-xl mt-sm relative overflow-hidden">
          <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-lg relative z-10">
            <div className="flex items-start gap-md">
              <div className="p-2 bg-error/10 rounded-lg text-error shrink-0 flex items-center justify-center">
                <span className="material-symbols-outlined">warning</span>
              </div>
              <div>
                <h3 className="font-headline-md text-[24px] font-bold text-error mb-1">Critical System Reset</h3>
                <p className="text-on-surface-variant text-body-sm max-w-xl">Resetting to factory settings will permanently delete all local agents, conversation history, and fine-tuning parameters. This action cannot be undone.</p>
              </div>
            </div>
            <Button
              className="px-xl py-md bg-error text-on-error rounded-xl font-label-md text-[14px] font-bold hover:bg-error/90 shadow-md active:scale-[0.98] transition-all whitespace-nowrap cursor-pointer"
              onClick={() => setShowResetConfirm(true)}
              disabled={resetting}
            >
              {resetting ? 'Resetting...' : 'Reset to Factory Settings'}
            </Button>
          </div>
        </div>
      </div>

      {/* System Logs Modal */}
      {showLogs && (
        <div role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 flex items-center justify-center z-50" onClick={() => setShowLogs(false)} onKeyDown={e => { if (e.key === 'Escape') setShowLogs(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl max-w-2xl w-full mx-lg shadow-2xl max-h-[80vh] overflow-y-auto" onClick={e => e.stopPropagation()}>
            <div className="flex justify-between items-center mb-lg">
              <h3 className="font-headline-md text-on-surface">System Logs</h3>
              <Button variant="ghost" className="cursor-pointer" onClick={() => setShowLogs(false)}>
                <span className="material-symbols-outlined">close</span>
              </Button>
            </div>
            <div className="bg-surface-container-high rounded-xl p-md font-mono text-label-sm text-on-surface-variant max-h-[50vh] overflow-y-auto">
              <p>Shannon Desktop v0.1.0</p>
              <p>System logs are available in the Tauri console.</p>
              <p className="mt-sm opacity-60">Run with RUST_LOG=debug for verbose output.</p>
            </div>
          </div>
        </div>
      )}

      {/* API Keys Modal */}
      {showApiKeys && (
        <div role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 flex items-center justify-center z-50" onClick={() => setShowApiKeys(false)} onKeyDown={e => { if (e.key === 'Escape') setShowApiKeys(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl max-w-lg w-full mx-lg shadow-2xl" onClick={e => e.stopPropagation()}>
            <div className="flex justify-between items-center mb-lg">
              <h3 className="font-headline-md text-on-surface">Manage API Keys</h3>
              <Button variant="ghost" className="cursor-pointer" onClick={() => setShowApiKeys(false)}>
                <span className="material-symbols-outlined">close</span>
              </Button>
            </div>
            <p className="text-body-sm text-on-surface-variant mb-md">API keys are managed through your provider's configuration. Update your key in Model Settings.</p>
            <Button className="w-full py-md bg-primary text-on-primary rounded-xl font-label-md cursor-pointer" onClick={() => setShowApiKeys(false)}>
              Go to Model Settings
            </Button>
          </div>
        </div>
      )}

      {/* Clear Cache Confirmation */}
      {showClearConfirm && (
        <div role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 backdrop-blur-sm flex items-center justify-center z-50" onClick={() => setShowClearConfirm(false)} onKeyDown={e => { if (e.key === 'Escape') setShowClearConfirm(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl shadow-xl border border-outline-variant/30 max-w-sm w-full mx-md" onClick={e => e.stopPropagation()}>
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-secondary text-[24px]">cleaning_services</span>
              <h3 className="font-headline-md text-on-surface">Clear Session Cache</h3>
            </div>
            <p className="text-body-md text-on-surface-variant mb-lg">This will clear all cached session data. Active sessions will not be affected.</p>
            <div className="flex justify-end gap-sm">
              <Button className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container" onClick={() => setShowClearConfirm(false)}>Cancel</Button>
              <Button className="px-lg py-sm rounded-xl bg-secondary text-on-secondary hover:bg-secondary/90" onClick={handleClearCache} disabled={clearing}>{clearing ? 'Clearing...' : 'Clear Cache'}</Button>
            </div>
          </div>
        </div>
      )}

      {/* Factory Reset Confirmation */}
      {showResetConfirm && (
        <div role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 backdrop-blur-sm flex items-center justify-center z-50" onClick={() => setShowResetConfirm(false)} onKeyDown={e => { if (e.key === 'Escape') setShowResetConfirm(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl shadow-xl border border-outline-variant/30 max-w-sm w-full mx-md" onClick={e => e.stopPropagation()}>
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-error text-[24px]">warning</span>
              <h3 className="font-headline-md text-on-surface">Factory Reset</h3>
            </div>
            <p className="text-body-md text-on-surface-variant mb-lg">This will permanently delete all local agents, conversation history, and configuration. This cannot be undone.</p>
            <div className="flex justify-end gap-sm">
              <Button className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container" onClick={() => setShowResetConfirm(false)}>Cancel</Button>
              <Button className="px-lg py-sm rounded-xl bg-error text-on-error hover:bg-error/90" onClick={handleFactoryReset} disabled={resetting}>{resetting ? 'Resetting...' : 'Reset'}</Button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
