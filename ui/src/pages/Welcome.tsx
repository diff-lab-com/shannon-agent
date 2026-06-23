import { useState, useEffect, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { open } from '@tauri-apps/plugin-dialog'
import * as api from '@/lib/tauri-api'
import { useApp } from '@/context/AppContext'
import { SIDEBAR_MODE_KEY } from '@/components/Sidebar'

// ─── Task taxonomy ──────────────────────────────────────────────────────────
// Drives Step 0 (primary use case). Each task carries a model recommendation
// and a tool preset surfaced in Step 2.
//
// `labelKey` / `blurbKey` resolve via react-intl; provider names and tool
// names are deliberately left untranslated (proper nouns).
type TaskId = 'code' | 'writing' | 'research' | 'general'

interface TaskOption {
  id: TaskId
  labelKey: string
  blurbKey: string
  icon: string
  recommendedProvider: string
  tools: string[]
}

const TASKS: TaskOption[] = [
  {
    id: 'code',
    labelKey: 'welcome.task.code.label',
    blurbKey: 'welcome.task.code.blurb',
    icon: 'code',
    recommendedProvider: 'anthropic',
    tools: ['filesystem', 'git', 'playwright'],
  },
  {
    id: 'writing',
    labelKey: 'welcome.task.writing.label',
    blurbKey: 'welcome.task.writing.blurb',
    icon: 'edit_note',
    recommendedProvider: 'anthropic',
    tools: ['web_search'],
  },
  {
    id: 'research',
    labelKey: 'welcome.task.research.label',
    blurbKey: 'welcome.task.research.blurb',
    icon: 'search',
    recommendedProvider: 'openai',
    tools: ['web_search', 'tavily'],
  },
  {
    id: 'general',
    labelKey: 'welcome.task.general.label',
    blurbKey: 'welcome.task.general.blurb',
    icon: 'auto_awesome',
    recommendedProvider: 'anthropic',
    tools: ['filesystem', 'web_search'],
  },
]

const PROVIDERS = [
  { id: 'anthropic', label: 'Anthropic', descKey: 'welcome.model.anthropic.desc' },
  { id: 'openai', label: 'OpenAI', descKey: 'welcome.model.openai.desc' },
  { id: 'ollama', label: 'Ollama', descKey: 'welcome.model.ollama.desc' },
  { id: 'deepseek', label: 'DeepSeek', descKey: 'welcome.model.deepseek.desc' },
] as const

const TOOL_CATALOG: Record<string, { labelKey: string; icon: string; descKey: string }> = {
  filesystem: { labelKey: 'welcome.tools.filesystem.label', icon: 'folder', descKey: 'welcome.tools.filesystem.desc' },
  git: { labelKey: 'welcome.tools.git.label', icon: 'commit', descKey: 'welcome.tools.git.desc' },
  playwright: { labelKey: 'welcome.tools.playwright.label', icon: 'web', descKey: 'welcome.tools.playwright.desc' },
  web_search: { labelKey: 'welcome.tools.webSearch.label', icon: 'travel_explore', descKey: 'welcome.tools.webSearch.desc' },
  tavily: { labelKey: 'welcome.tools.tavily.label', icon: 'menu_book', descKey: 'welcome.tools.tavily.desc' },
}

const SHORTCUT_ROWS = [
  { keys: '⌘ K', actionKey: 'shortcuts.openPalette' },
  { keys: '⌘ N', actionKey: 'shortcuts.newChat' },
  { keys: '⌘ 1 / 2 / 3', actionKey: 'shortcuts.jumpTabs' },
  { keys: '?', actionKey: 'shortcuts.showAll' },
  { keys: 'Esc', actionKey: 'shortcuts.cancel' },
] as const

// ─── Documents skill recommendations (P2.4) ─────────────────────────────────
// Instead of building a Documents engine inside Shannon (Phase D's MVP), we
// surface host-side Documents skills the user can install with one click.
// Each entry maps to a GitHub repo that `install_skill_from_repo` clones into
// `~/.shannon/skills/`. The catalog is deliberately short — only the most
// universally useful Documents skills. Repos are placeholders until the
// matching shannon-skills-* repos are published; the install UI gracefully
// reports failure so users can retry once a repo goes live.
interface DocumentsSkill {
  id: string
  labelKey: string
  descKey: string
  icon: string
  repo: string
  ref: string
}

const DOCUMENTS_SKILLS: DocumentsSkill[] = [
  {
    id: 'pandoc-docx',
    labelKey: 'welcome.skills.pandoc.label',
    descKey: 'welcome.skills.pandoc.desc',
    icon: 'description',
    repo: 'shannon-agent/shannon-skills-docs',
    ref: 'main',
  },
  {
    id: 'python-docx',
    labelKey: 'welcome.skills.pydocx.label',
    descKey: 'welcome.skills.pydocx.desc',
    icon: 'data_object',
    repo: 'shannon-agent/shannon-skills-docs',
    ref: 'main',
  },
  {
    id: 'markdown-beautify',
    labelKey: 'welcome.skills.beautify.label',
    descKey: 'welcome.skills.beautify.desc',
    icon: 'auto_fix_high',
    repo: 'shannon-agent/shannon-skills-docs',
    ref: 'main',
  },
]

export const WELCOME_SEEN_KEY = 'shannon.hasSeenWelcome'

export function shouldShowWelcome(loading: boolean, hasProvider: boolean): boolean {
  if (typeof window === 'undefined') return false
  if (loading) return false
  const seen = window.localStorage.getItem(WELCOME_SEEN_KEY)
  return !seen && !hasProvider
}

export function markWelcomeSeen() {
  window.localStorage.setItem(WELCOME_SEEN_KEY, '1')
}

// Display labels for the Stepper (Step 0..3). Order matches STEPS_INDEX.
const STEP_LABEL_KEYS = [
  'welcome.step.task',
  'welcome.step.model',
  'welcome.step.tools',
  'welcome.step.done',
] as const

export default function Welcome() {
  const intl = useIntl()
  const navigate = useNavigate()
  const { refreshConfig, refreshStatus, config } = useApp()
  const [step, setStep] = useState(0)
  const [task, setTask] = useState<TaskId>('general')
  const [provider, setProvider] = useState<string>('anthropic')
  const [apiKey, setApiKey] = useState('')
  const [saving, setSaving] = useState(false)
  const [pickedDir, setPickedDir] = useState<string | null>(null)
  const [enabledTools, setEnabledTools] = useState<Record<string, boolean>>({})
  const [devMode, setDevMode] = useState(false)
  const [testing, setTesting] = useState(false)
  const [envHasKey, setEnvHasKey] = useState(false)
  const envCheckedRef = useRef(false)
  const [skillState, setSkillState] = useState<
    Record<string, { status: 'idle' | 'installing' | 'installed' | 'failed'; error?: string }>
  >({})

  const currentTask = TASKS.find(t => t.id === task)!
  const recommendedProvider = PROVIDERS.find(p => p.id === currentTask.recommendedProvider)

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
          toast.info(intl.formatMessage({ id: 'welcome.envDetected.ollama' }))
        } else if (detected.has_api_key) {
          setEnvHasKey(true)
          const label = PROVIDERS.find(p => p.id === detected.provider)?.label ?? detected.provider
          toast.success(intl.formatMessage({ id: 'welcome.envDetected.toast' }, { provider: label }))
        }
      })
      .catch(e => console.warn('detectProviderFromEnv failed:', e))
  }, [intl])

  const runTestConnection = async () => {
    if (provider === 'ollama') return
    setTesting(true)
    try {
      const result = await api.testProviderConnection(provider, apiKey)
      const label = PROVIDERS.find(p => p.id === provider)?.label ?? provider
      switch (result.kind) {
        case 'success':
          toast.success(intl.formatMessage({ id: 'welcome.testConnection.success' }, { provider: label }))
          return
        case 'invalid_key':
          toast.error(intl.formatMessage({ id: 'welcome.testConnection.invalidKey' }))
          return
        case 'rate_limited':
          toast.warning(intl.formatMessage({ id: 'welcome.testConnection.rateLimited' }))
          return
        case 'provider_error':
          toast.error(intl.formatMessage({ id: 'welcome.testConnection.providerError' }, { provider: label, status: result.status }))
          return
        case 'network_unreachable':
          toast.error(intl.formatMessage({ id: 'welcome.testConnection.networkUnreachable' }, { provider: label }))
          return
        case 'unknown':
          toast.error(intl.formatMessage({ id: 'welcome.testConnection.unknown' }, { message: result.message }))
          return
      }
    } catch (e) {
      console.warn('testProviderConnection failed:', e)
      toast.error(intl.formatMessage({ id: 'welcome.testConnection.failed' }))
    } finally {
      setTesting(false)
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
          console.warn('configure working_dir failed:', e)
          toast.error(intl.formatMessage({ id: 'welcome.toast.workingDir.failed' }))
        }
      }
    } catch (e) {
      console.warn('Welcome folder picker failed:', e)
      toast.error(intl.formatMessage({ id: 'welcome.toast.folderPicker.failed' }))
    }
  }

  const handleModelSubmit = async () => {
    setSaving(true)
    try {
      if (provider !== 'ollama' && apiKey) {
        await api.configure({ key: 'api_key', value: apiKey })
      }
      await api.switchProvider({ provider, model: '' }).catch(e => console.warn('switchProvider in welcome:', e))
      await Promise.all([refreshConfig(), refreshStatus()])
      // Pre-check tools recommended for this task so the user can opt in/out.
      const initial: Record<string, boolean> = {}
      for (const t of currentTask.tools) initial[t] = true
      setEnabledTools(prev => ({ ...initial, ...prev }))
      setStep(2)
    } catch (e) {
      console.warn('Welcome provider setup failed:', e)
      toast.error(intl.formatMessage({ id: 'welcome.toast.provider.failed' }))
    }
    setSaving(false)
  }

  const toggleTool = (id: string) => {
    setEnabledTools(prev => ({ ...prev, [id]: !prev[id] }))
  }

  const enabledToolCount = Object.values(enabledTools).filter(Boolean).length

  const installDocumentsSkill = async (skill: DocumentsSkill) => {
    setSkillState(prev => ({ ...prev, [skill.id]: { status: 'installing' } }))
    try {
      await api.installSkillFromRepo(skill.id, skill.repo, skill.ref)
      setSkillState(prev => ({ ...prev, [skill.id]: { status: 'installed' } }))
      toast.success(intl.formatMessage({ id: 'welcome.skills.toast.installed' }, { name: intl.formatMessage({ id: skill.labelKey }) }))
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setSkillState(prev => ({ ...prev, [skill.id]: { status: 'failed', error: msg } }))
      toast.error(intl.formatMessage({ id: 'welcome.skills.toast.failed' }, { name: intl.formatMessage({ id: skill.labelKey }) }))
    }
  }

  return (
    <div className="min-h-screen bg-background text-on-surface flex flex-col">
      <header className="flex items-center justify-between px-xl py-lg">
        <div className="flex items-center gap-sm">
          <span className="material-symbols-outlined text-primary">auto_awesome</span>
          <span className="font-headline-md text-on-surface">{intl.formatMessage({ id: 'app.name' })}</span>
        </div>
        <button
          onClick={finish}
          className="font-label-md text-on-surface-variant hover:text-primary cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded px-xs"
          aria-label={intl.formatMessage({ id: 'welcome.skipAria' })}
        >
          {intl.formatMessage({ id: 'welcome.skip' })}
        </button>
      </header>

      <main className="flex-1 flex items-center justify-center px-xl py-xl">
        <div className="w-full max-w-xl">
          <Stepper step={step} />

          {/* Step 0: Task */}
          {step === 0 && (
            <Card
              title={intl.formatMessage({ id: 'welcome.task.title' })}
              subtitle={intl.formatMessage({ id: 'welcome.task.subtitle' })}
              footer={
                <>
                  <span />
                  <button
                    onClick={() => {
                      // Default provider to the task recommendation when advancing.
                      setProvider(currentTask.recommendedProvider)
                      setStep(1)
                    }}
                    className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                  >
                    {intl.formatMessage({ id: 'welcome.task.continue' })}
                  </button>
                </>
              }
            >
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-sm mb-lg">
                {TASKS.map(t => (
                  <button
                    key={t.id}
                    onClick={() => setTask(t.id)}
                    aria-pressed={task === t.id}
                    className={`text-left p-md rounded-xl border cursor-pointer transition-all focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary ${
                      task === t.id
                        ? 'border-2 border-primary bg-primary-container/5'
                        : 'border-outline-variant/50 hover:border-primary/50'
                    }`}
                  >
                    <div className="flex items-start gap-sm">
                      <span className="material-symbols-outlined text-primary shrink-0">{t.icon}</span>
                      <div className="flex-1">
                        <div className="font-headline-md text-on-surface">{intl.formatMessage({ id: t.labelKey })}</div>
                        <div className="font-body-sm text-on-surface-variant mt-xs">{intl.formatMessage({ id: t.blurbKey })}</div>
                      </div>
                      <div className={`w-5 h-5 rounded-full border-2 shrink-0 ${task === t.id ? 'border-primary bg-primary' : 'border-outline-variant'}`} />
                    </div>
                  </button>
                ))}
              </div>
            </Card>
          )}

          {/* Step 1: Model */}
          {step === 1 && (
            <Card
              title={intl.formatMessage({ id: 'welcome.model.title' })}
              subtitle={
                recommendedProvider
                  ? intl.formatMessage(
                      { id: 'welcome.model.subtitle.recommended' },
                      { task: intl.formatMessage({ id: currentTask.labelKey }), provider: recommendedProvider.label },
                    )
                  : intl.formatMessage({ id: 'welcome.model.subtitle.default' })
              }
              footer={
                <>
                  <button onClick={() => setStep(0)} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
                    {intl.formatMessage({ id: 'welcome.model.back' })}
                  </button>
                  <button
                    onClick={handleModelSubmit}
                    disabled={saving || (provider !== 'ollama' && !apiKey && !envHasKey)}
                    className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed hover:bg-primary/90 transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                  >
                    {saving
                      ? intl.formatMessage({ id: 'welcome.model.saving' })
                      : intl.formatMessage({ id: 'welcome.model.continue' })}
                  </button>
                </>
              }
            >
              <div className="space-y-sm mb-lg">
                {PROVIDERS.map(p => (
                  <button
                    key={p.id}
                    onClick={() => setProvider(p.id)}
                    aria-pressed={provider === p.id}
                    className={`w-full text-left p-md rounded-xl border cursor-pointer transition-all focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary ${
                      provider === p.id
                        ? 'border-2 border-primary bg-primary-container/5'
                        : 'border-outline-variant/50 hover:border-primary/50'
                    }`}
                  >
                    <div className="flex items-center justify-between gap-sm">
                      <div>
                        <div className="font-headline-md text-on-surface">{p.label}</div>
                        <div className="font-body-sm text-on-surface-variant">{intl.formatMessage({ id: p.descKey })}</div>
                      </div>
                      <div className={`w-5 h-5 rounded-full border-2 shrink-0 ${provider === p.id ? 'border-primary bg-primary' : 'border-outline-variant'}`} />
                    </div>
                  </button>
                ))}
              </div>
              {provider !== 'ollama' && (
                <div className="mb-lg">
                  <label htmlFor="welcome-api-key" className="font-label-md text-on-surface-variant block mb-xs">
                    {intl.formatMessage({ id: 'welcome.model.apiKey.label' })}
                  </label>
                  <input
                    id="welcome-api-key"
                    type="password"
                    value={apiKey}
                    onChange={e => setApiKey(e.target.value)}
                    placeholder={envHasKey ? '(loaded from environment)' : 'sk-...'}
                    autoComplete="off"
                    className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm"
                  />
                  <div className="flex items-center justify-between gap-md mt-xs">
                    <p className="font-label-sm text-on-surface-variant flex-1">
                      {intl.formatMessage({ id: 'welcome.model.apiKey.help' })}
                    </p>
                    <button
                      type="button"
                      onClick={runTestConnection}
                      disabled={testing || !apiKey}
                      className="shrink-0 px-md py-xs rounded-lg font-label-md cursor-pointer transition-all bg-surface-container-low hover:bg-surface-container-high border border-outline-variant/50 text-on-surface disabled:opacity-50 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary flex items-center gap-xs"
                    >
                      {testing && <span className="material-symbols-outlined text-[16px] animate-spin">progress_activity</span>}
                      {testing
                        ? intl.formatMessage({ id: 'welcome.testConnection.testing' })
                        : intl.formatMessage({ id: 'welcome.testConnection.button' })}
                    </button>
                  </div>
                </div>
              )}
            </Card>
          )}

          {/* Step 2: Tools */}
          {step === 2 && (
            <Card
              title={intl.formatMessage({ id: 'welcome.tools.title' })}
              subtitle={intl.formatMessage({ id: 'welcome.tools.subtitle' })}
              footer={
                <>
                  <button onClick={() => setStep(1)} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
                    {intl.formatMessage({ id: 'welcome.model.back' })}
                  </button>
                  <button
                    onClick={() => setStep(3)}
                    className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                  >
                    {intl.formatMessage({ id: 'welcome.tools.continue' })}
                  </button>
                </>
              }
            >
              <div className="space-y-sm mb-lg">
                {Object.entries(TOOL_CATALOG).map(([id, meta]) => {
                  const enabled = !!enabledTools[id]
                  const recommended = currentTask.tools.includes(id)
                  const toolLabel = intl.formatMessage({ id: meta.labelKey })
                  return (
                    <label
                      key={id}
                      className={`flex items-start gap-md p-md rounded-xl border cursor-pointer transition-all ${
                        enabled ? 'border-2 border-primary bg-primary-container/5' : 'border-outline-variant/50 hover:border-primary/50'
                      }`}
                    >
                      <input
                        type="checkbox"
                        checked={enabled}
                        onChange={() => toggleTool(id)}
                        className="mt-xs accent-primary"
                        aria-label={intl.formatMessage({ id: 'welcome.tools.enableAria' }, { label: toolLabel })}
                      />
                      <span className="material-symbols-outlined text-on-surface-variant shrink-0">{meta.icon}</span>
                      <div className="flex-1">
                        <div className="flex items-center gap-xs">
                          <span className="font-headline-md text-on-surface">{toolLabel}</span>
                          {recommended && (
                            <span className="text-[10px] uppercase tracking-wider font-bold text-primary bg-primary/10 px-1.5 py-0.5 rounded">
                              {intl.formatMessage({ id: 'welcome.tools.recommended' })}
                            </span>
                          )}
                        </div>
                        <div className="font-body-sm text-on-surface-variant mt-xs">{intl.formatMessage({ id: meta.descKey })}</div>
                      </div>
                    </label>
                  )
                })}
              </div>
              <p className="font-body-sm text-on-surface-variant">
                {intl.formatMessage(
                  { id: 'welcome.tools.workingDir.help' },
                  {
                    link: (chunks: React.ReactNode) => (
                      <button
                        onClick={() => { markWelcomeSeen(); navigate('/settings/general') }}
                        className="text-primary hover:underline cursor-pointer"
                      >
                        {chunks}
                      </button>
                    ),
                  },
                )}
              </p>
            </Card>
          )}

          {/* Step 3: Done */}
          {step === 3 && (
            <Card
              title={intl.formatMessage({ id: 'welcome.done.title' })}
              subtitle={intl.formatMessage({ id: 'welcome.done.subtitle' })}
              footer={
                <>
                  <button onClick={() => setStep(2)} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
                    {intl.formatMessage({ id: 'welcome.done.back' })}
                  </button>
                  <button
                    onClick={finish}
                    className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                  >
                    {intl.formatMessage({ id: 'welcome.done.start' })}
                  </button>
                </>
              }
            >
              {/* Summary */}
              <div className="bg-surface-container-low rounded-xl p-md mb-md">
                <div className="font-label-sm text-on-surface-variant mb-xs">{intl.formatMessage({ id: 'welcome.done.setup.label' })}</div>
                <ul className="space-y-xs text-body-sm text-on-surface">
                  <li className="flex items-center gap-sm">
                    <span className="material-symbols-outlined text-[18px] text-primary">{currentTask.icon}</span>
                    <span>{intl.formatMessage({ id: currentTask.labelKey })}</span>
                  </li>
                  <li className="flex items-center gap-sm">
                    <span className="material-symbols-outlined text-[18px] text-primary">memory</span>
                    <span>{PROVIDERS.find(p => p.id === provider)?.label ?? provider}</span>
                  </li>
                  <li className="flex items-center gap-sm">
                    <span className="material-symbols-outlined text-[18px] text-primary">build</span>
                    <span>{intl.formatMessage({ id: 'welcome.done.setup.tools' }, { count: enabledToolCount })}</span>
                  </li>
                </ul>
              </div>

              {/* Optional workspace picker */}
              <div className="bg-surface-container-low rounded-xl p-md mb-md">
                <div className="font-label-sm text-on-surface-variant mb-xs">{intl.formatMessage({ id: 'welcome.done.workingDir.label' })}</div>
                <div className="font-mono text-on-surface text-sm break-all mb-sm">
                  {pickedDir ?? config?.working_dir ?? intl.formatMessage({ id: 'welcome.done.workingDir.default' })}
                </div>
                <button
                  onClick={pickDirectory}
                  className="px-md py-sm bg-surface-container-low hover:bg-surface-container-high border border-outline-variant/50 rounded-lg font-label-md text-on-surface cursor-pointer transition-colors flex items-center gap-sm focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                >
                  <span className="material-symbols-outlined text-[18px]">folder_open</span>
                  {pickedDir
                    ? intl.formatMessage({ id: 'welcome.done.workingDir.chooseOther' })
                    : intl.formatMessage({ id: 'welcome.done.workingDir.choose' })}
                </button>
              </div>

              {/* Shortcuts */}
              <div className="space-y-sm">
                <div className="font-label-md text-on-surface-variant mb-xs">{intl.formatMessage({ id: 'welcome.done.shortcuts.label' })}</div>
                {SHORTCUT_ROWS.map(s => (
                  <div key={s.keys} className="flex items-center justify-between py-xs">
                    <span className="font-body-sm text-on-surface-variant">{intl.formatMessage({ id: s.actionKey })}</span>
                    <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono shrink-0">{s.keys}</kbd>
                  </div>
                ))}
              </div>
              <p className="font-body-sm text-on-surface-variant mt-md">
                {intl.formatMessage(
                  { id: 'welcome.done.shortcuts.help' },
                  {
                    key: (chunks: React.ReactNode) => (
                      <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono">{chunks}</kbd>
                    ),
                  },
                )}
              </p>

              {/* Developer mode opt-in */}
              <label className="mt-md flex items-start gap-sm p-md rounded-xl border border-outline-variant/50 hover:border-primary/50 cursor-pointer transition-all">
                <input
                  type="checkbox"
                  checked={devMode}
                  onChange={() => setDevMode(v => !v)}
                  className="mt-xs accent-primary"
                  aria-label={intl.formatMessage({ id: 'welcome.done.devMode.aria' })}
                />
                <div>
                  <div className="font-headline-md text-on-surface">{intl.formatMessage({ id: 'welcome.done.devMode.title' })}</div>
                  <div className="font-body-sm text-on-surface-variant mt-xs">
                    {intl.formatMessage({ id: 'welcome.done.devMode.desc' })}
                  </div>
                </div>
              </label>

              {/* P2.4 — Documents skill recommendations. Optional one-click
                  install; users can skip and install later from Extensions Hub. */}
              <div className="mt-md p-md rounded-xl border border-outline-variant/50 bg-surface-container-low">
                <div className="flex items-center gap-xs mb-xs">
                  <span className="material-symbols-outlined text-primary text-[20px]">extension</span>
                  <span className="font-headline-md text-on-surface">
                    {intl.formatMessage({ id: 'welcome.skills.title' })}
                  </span>
                </div>
                <p className="font-body-sm text-on-surface-variant mb-md">
                  {intl.formatMessage({ id: 'welcome.skills.subtitle' })}
                </p>
                <ul className="space-y-sm">
                  {DOCUMENTS_SKILLS.map(skill => {
                    const state = skillState[skill.id] ?? { status: 'idle' as const }
                    return (
                      <li
                        key={skill.id}
                        className="flex items-start gap-sm p-sm rounded-lg bg-surface-container-lowest border border-outline-variant/30"
                      >
                        <span className="material-symbols-outlined text-on-surface-variant text-[20px] mt-[2px] shrink-0">
                          {skill.icon}
                        </span>
                        <div className="flex-1 min-w-0">
                          <div className="font-label-md text-on-surface">{intl.formatMessage({ id: skill.labelKey })}</div>
                          <div className="font-body-sm text-on-surface-variant mt-[2px]">
                            {intl.formatMessage({ id: skill.descKey })}
                          </div>
                          {state.status === 'failed' && state.error && (
                            <div className="font-body-sm text-error mt-xs">{state.error}</div>
                          )}
                        </div>
                        <button
                          type="button"
                          onClick={() => installDocumentsSkill(skill)}
                          disabled={state.status === 'installing' || state.status === 'installed'}
                          className="shrink-0 px-md py-xs rounded-lg font-label-md text-label-sm bg-surface-container-high hover:bg-surface-container-highest border border-outline-variant/50 text-on-surface cursor-pointer transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-xs"
                          aria-label={intl.formatMessage({ id: 'welcome.skills.install.aria' }, { name: intl.formatMessage({ id: skill.labelKey }) })}
                        >
                          {state.status === 'installing' && (
                            <span className="material-symbols-outlined text-[14px] animate-spin">progress_activity</span>
                          )}
                          {state.status === 'installed' ? (
                            <span className="material-symbols-outlined text-[14px]">check</span>
                          ) : state.status === 'installing' ? (
                            intl.formatMessage({ id: 'welcome.skills.installing' })
                          ) : (
                            intl.formatMessage({ id: 'welcome.skills.install' })
                          )}
                        </button>
                      </li>
                    )
                  })}
                </ul>
                <p className="font-body-sm text-on-surface-variant mt-md">
                  {intl.formatMessage(
                    { id: 'welcome.skills.later' },
                    {
                      link: (chunks: React.ReactNode) => (
                        <button
                          onClick={() => { markWelcomeSeen(); navigate('/extensions/featured') }}
                          className="text-primary hover:underline cursor-pointer"
                        >
                          {chunks}
                        </button>
                      ),
                    },
                  )}
                </p>
              </div>
            </Card>
          )}
        </div>
      </main>
    </div>
  )
}

function Stepper({ step }: { step: number }) {
  const intl = useIntl()
  const stepLabel = intl.formatMessage({ id: STEP_LABEL_KEYS[step] })
  return (
    <div
      className="flex items-center justify-center gap-sm mb-xl"
      aria-label={intl.formatMessage(
        { id: 'welcome.stepper.step' },
        { current: step + 1, total: STEP_LABEL_KEYS.length, label: stepLabel },
      )}
    >
      {STEP_LABEL_KEYS.map((key, i) => (
        <div key={key} className="flex items-center gap-sm">
          <div className={`w-2 h-2 rounded-full ${i <= step ? 'bg-primary' : 'bg-outline-variant'}`} />
          {i === step && <span className="font-label-sm text-primary">{stepLabel}</span>}
          {i < STEP_LABEL_KEYS.length - 1 && <div className="w-8 h-px bg-outline-variant" aria-hidden="true" />}
        </div>
      ))}
    </div>
  )
}

function Card({ title, subtitle, footer, children }: {
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
