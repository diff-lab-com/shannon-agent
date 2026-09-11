import { useState, useEffect, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { toastError } from '@/lib/errorToast'
import { open } from '@tauri-apps/plugin-dialog'
import * as api from '@/lib/tauri-api'
import { useCatalog } from '@/context/CatalogContext'
import { SIDEBAR_MODE_KEY } from '@/components/Sidebar'
import AddProviderModal from '@/components/settings/AddProviderModal'
import { GradientText } from '@/components/reactbits/GradientText'
import { Stepper } from './welcome/components'
import { TaskStep } from './welcome/TaskStep'
import { ModelStep } from './welcome/ModelStep'
import { DoneStep } from './welcome/DoneStep'
import MigrationWizard from '@/components/migration/MigrationWizard'
import { TASKS, type TaskId, type DocumentsSkill } from './welcome/constants'
import type { ProvidersFile } from '@/types'

export const WELCOME_SEEN_KEY = 'shannon.hasSeenWelcome'

// Two-step onboarding (UI audit §3.1: competitors onboard in 2 screens).
// Screen 1 combines task + model; tools use the task's recommended defaults
// (prefilled on advance — ToolsStep remains available from Settings).
const WELCOME_STEP_LABELS = ['welcome.step.task', 'welcome.step.done']

export function shouldShowWelcome(loading: boolean, hasProvider: boolean): boolean {
  if (typeof window === 'undefined') return false
  if (loading) return false
  const seen = window.localStorage.getItem(WELCOME_SEEN_KEY)
  return !seen && !hasProvider
}

export function markWelcomeSeen() {
  window.localStorage.setItem(WELCOME_SEEN_KEY, '1')
}

export default function Welcome() {
  const intl = useIntl()
  const navigate = useNavigate()
  const { refreshConfig, refreshStatus, config } = useCatalog()
  const [step, setStep] = useState(0)
  // No default selection — step 0 asks the user to make an explicit
  // choice, so Continue stays disabled until a card is picked.
  const [task, setTask] = useState<TaskId | null>(null)
  const [provider, setProvider] = useState<string>('anthropic')
  const [saving, setSaving] = useState(false)
  const [pickedDir, setPickedDir] = useState<string | null>(null)
  const [enabledTools, setEnabledTools] = useState<Record<string, boolean>>({})
  const [devMode, setDevMode] = useState(false)
  // True once a usable provider was detected from the environment (API key
  // present, or a local engine like Ollama that needs no key).
  const [envProviderReady, setEnvProviderReady] = useState(false)
  const envCheckedRef = useRef(false)
  const [providerSaved, setProviderSaved] = useState(false)
  const [showAddProviderModal, setShowAddProviderModal] = useState(false)
  const [skillState, setSkillState] = useState<
    Record<string, { status: 'idle' | 'installing' | 'installed' | 'failed'; error?: string }>
  >({})
  // P1-6 — migration wizard overlay (import from Claude Code / ZCode),
  // reachable from the final Welcome step.
  const [migrationOpen, setMigrationOpen] = useState(false)

  // task is always one of the TaskId union, so the lookup can only miss on
  // programmer error — fall back to the first task instead of asserting.
  const currentTask = TASKS.find(t => t.id === task) ?? TASKS[0]
  // A working env-detected provider is enough to move on, even when it is
  // not the task's recommendation — the recommendation is a default, not a
  // gate (previously this dead-ended env-key users on non-recommended
  // providers and forced manual API-key entry).
  const canAdvanceFromModel = providerSaved || envProviderReady
  const enabledToolCount = Object.values(enabledTools).filter(Boolean).length

  // On mount, probe the shell for a pre-configured provider so the user can
  // skip the API-key entry step. Only fires once — the ref guards against
  // StrictMode double-invoke in dev.
  useEffect(() => {
    if (envCheckedRef.current) return
    envCheckedRef.current = true
    api.detectProviderFromEnv()
      .then(detected => {
        if (!detected) return
        setProvider(detected.provider)
        if (detected.provider === 'ollama') {
          // Ollama runs locally — detected means usable, no key involved.
          setEnvProviderReady(true)
          toast.info(intl.formatMessage({ id: 'welcome.envDetected.ollama' }))
        } else if (detected.has_api_key) {
          setEnvProviderReady(true)
          toast.success(intl.formatMessage({ id: 'welcome.envDetected.toast' }, { provider: detected.provider }))
        }
      })
      .catch(e => console.warn('detectProviderFromEnv failed:', e))
  }, [intl])

  const handleAddProviderSaved = async (f: ProvidersFile) => {
    const targetId = f.active_provider_id ?? f.providers[f.providers.length - 1]?.id
    if (!targetId) {
      toastError(intl.formatMessage({ id: 'welcome.toast.provider.failed' }), new Error('No provider id returned'))
      return
    }
    setSaving(true)
    try {
      await api.setActiveProvider(targetId)
      await Promise.all([refreshConfig(), refreshStatus()])
      const active = f.providers.find(p => p.id === targetId)
      if (active) {
        setProvider(active.kind)
      }
      // Pre-check tools recommended for this task so the user can opt in/out.
      const initial: Record<string, boolean> = {}
      for (const t of currentTask.tools) initial[t] = true
      setEnabledTools(prev => ({ ...initial, ...prev }))
      setShowAddProviderModal(false)
      setProviderSaved(true)
      setStep(1)
    } catch (e) {
      toastError(intl.formatMessage({ id: 'welcome.toast.provider.failed' }), e)
    } finally {
      setSaving(false)
    }
  }

  const finish = async () => {
    markWelcomeSeen()
    if (devMode) {
      window.localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
    }
    // Seed sample tasks on first run so Tasks / Today isn't empty. Idempotent
    // backend-side; failure is non-fatal — just log and continue.
    try {
      await api.seedSampleData()
    } catch (e) {
      console.warn('seedSampleData failed:', e)
    }
    navigate('/chat', { replace: true })
  }

  const pickDirectory = async () => {
    try {
      const sel = await open({ directory: true, multiple: false })
      if (typeof sel === 'string') {
        setPickedDir(sel)
        try {
          await api.configure({ key: 'working_dir', value: sel })
          await refreshConfig()
          toast.success(intl.formatMessage({ id: 'welcome.toast.workingDir.updated' }))
        } catch (e) {
          toastError(intl.formatMessage({ id: 'welcome.toast.workingDir.failed' }), e)
        }
      }
    } catch (e) {
      toastError(intl.formatMessage({ id: 'welcome.toast.folderPicker.failed' }), e)
    }
  }

  const advanceFromModel = () => {
    if (!canAdvanceFromModel) return
    // Pre-check tools recommended for this task so the user can opt in/out
    // later from Settings — the dedicated tools screen is gone (2-step flow).
    const initial: Record<string, boolean> = {}
    for (const t of currentTask.tools) initial[t] = true
    setEnabledTools(prev => ({ ...initial, ...prev }))
    setStep(1)
  }

  const installDocumentsSkill = async (skill: DocumentsSkill) => {
    setSkillState(prev => ({ ...prev, [skill.id]: { status: 'installing' } }))
    try {
      await api.installSkillFromRepo(skill.id, skill.repo, skill.ref)
      setSkillState(prev => ({ ...prev, [skill.id]: { status: 'installed' } }))
      toast.success(intl.formatMessage({ id: 'welcome.skills.toast.installed' }, { name: intl.formatMessage({ id: skill.labelKey }) }))
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setSkillState(prev => ({ ...prev, [skill.id]: { status: 'failed', error: msg } }))
      toastError(intl.formatMessage({ id: 'welcome.skills.toast.failed' }, { name: intl.formatMessage({ id: skill.labelKey }) }), e)
    }
  }

  const openFeaturedSkills = () => {
    markWelcomeSeen()
    navigate('/extensions/featured')
  }

  return (
    <div className="min-h-screen bg-background text-on-surface flex flex-col">
      <header className="flex items-center justify-between px-xl py-lg">
        <div className="flex items-center gap-sm">
          <span className="material-symbols-outlined text-primary">auto_awesome</span>
          <GradientText
            text={intl.formatMessage({ id: 'app.name' })}
            className="font-headline-md"
          />
        </div>
        <Button
          variant="ghost"
          onClick={finish}
          className="font-label-md text-on-surface-variant hover:text-primary cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded px-xs"
          aria-label={intl.formatMessage({ id: 'welcome.skipAria' })}
        >
          {intl.formatMessage({ id: 'welcome.skip' })}
        </Button>
      </header>

      <main className="flex-1 flex items-center justify-center px-xl py-xl">
        <div className="w-full max-w-xl">
          <Stepper step={step} labels={WELCOME_STEP_LABELS} />

          {step === 0 && (
            <>
              <TaskStep
                task={task}
                setTask={setTask}
                onContinue={() => {
                  // Task picked: guide the eye down to the model section
                  // instead of navigating away (single-screen flow).
                  document.getElementById('welcome-model-section')
                    ?.scrollIntoView({ behavior: 'smooth', block: 'start' })
                }}
              />
              {task !== null && (
                <div id="welcome-model-section" className="mt-lg scroll-mt-lg">
                  <ModelStep
                    task={task}
                    saving={saving}
                    canContinue={canAdvanceFromModel}
                    onOpenAddProvider={() => setShowAddProviderModal(true)}
                    onBack={() => setTask(null)}
                    onContinue={advanceFromModel}
                  />
                </div>
              )}
            </>
          )}

          {step === 1 && task !== null && (
            <DoneStep
              task={task}
              provider={provider}
              enabledToolCount={enabledToolCount}
              pickedDir={pickedDir}
              fallbackWorkingDir={config?.working_dir ?? null}
              devMode={devMode}
              setDevMode={setDevMode}
              skillState={skillState}
              onPickDirectory={pickDirectory}
              onBack={() => setStep(0)}
              onFinish={finish}
              onInstallSkill={installDocumentsSkill}
              onBrowseFeaturedSkills={openFeaturedSkills}
              onOpenMigration={() => setMigrationOpen(true)}
            />
          )}
        </div>
      </main>

      {migrationOpen && (
        <MigrationWizard open={migrationOpen} onClose={() => setMigrationOpen(false)} />
      )}

      {showAddProviderModal && (
        <AddProviderModal
          editing={null}
          onClose={() => setShowAddProviderModal(false)}
          onSaved={handleAddProviderSaved}
        />
      )}
    </div>
  )
}