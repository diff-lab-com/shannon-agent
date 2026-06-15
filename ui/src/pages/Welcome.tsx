import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { open } from '@tauri-apps/plugin-dialog'
import * as api from '@/lib/tauri-api'
import { useApp } from '@/context/AppContext'
import { SIDEBAR_MODE_KEY } from '@/components/Sidebar'

// ─── Task taxonomy ──────────────────────────────────────────────────────────
// Drives Step 0 (primary use case). Each task carries a model recommendation
// and a tool preset surfaced in Step 2.
type TaskId = 'code' | 'writing' | 'research' | 'general'

interface TaskOption {
  id: TaskId
  label: string
  icon: string
  blurb: string
  recommendedProvider: string
  tools: string[]
}

const TASKS: TaskOption[] = [
  {
    id: 'code',
    label: 'Code',
    icon: 'code',
    blurb: 'Build apps, write scripts, debug and refactor.',
    recommendedProvider: 'anthropic',
    tools: ['filesystem', 'git', 'playwright'],
  },
  {
    id: 'writing',
    label: 'Writing',
    icon: 'edit_note',
    blurb: 'Draft docs, articles, posts, and emails.',
    recommendedProvider: 'anthropic',
    tools: ['web_search'],
  },
  {
    id: 'research',
    label: 'Research',
    icon: 'search',
    blurb: 'Search, summarize, and cite sources.',
    recommendedProvider: 'openai',
    tools: ['web_search', 'tavily'],
  },
  {
    id: 'general',
    label: 'General',
    icon: 'auto_awesome',
    blurb: 'A bit of everything — start here if unsure.',
    recommendedProvider: 'anthropic',
    tools: ['filesystem', 'web_search'],
  },
]

const PROVIDERS = [
  { id: 'anthropic', label: 'Anthropic', description: 'Claude Sonnet 4.6 — recommended for coding' },
  { id: 'openai', label: 'OpenAI', description: 'GPT-4o / o1 — strong reasoning, multi-modal' },
  { id: 'ollama', label: 'Ollama', description: 'Local models, no API key needed' },
  { id: 'deepseek', label: 'DeepSeek', description: 'Cost-effective, long-context tasks' },
] as const

const TOOL_CATALOG: Record<string, { label: string; icon: string; description: string }> = {
  filesystem: { label: 'Filesystem', icon: 'folder', description: 'Read and write files in your workspace.' },
  git: { label: 'Git', icon: 'commit', description: 'Branch, commit, diff, and merge.' },
  playwright: { label: 'Playwright', icon: 'web', description: 'Automate the browser for end-to-end tests.' },
  web_search: { label: 'Web Search', icon: 'travel_explore', description: 'Look up current information on the web.' },
  tavily: { label: 'Tavily Research', icon: 'menu_book', description: 'Cited research with deeper extraction.' },
}

const TOP_SHORTCUTS = [
  { keys: '⌘ K', action: 'Open command palette' },
  { keys: '⌘ N', action: 'New chat' },
  { keys: '⌘ 1 / 2 / 3', action: 'Jump to Chat / Projects / Scheduled' },
  { keys: '?', action: 'Show all shortcuts' },
  { keys: 'Esc', action: 'Cancel running query' },
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

const STEPS = ['Task', 'Model', 'Tools', 'Done'] as const

export default function Welcome() {
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

  const currentTask = TASKS.find(t => t.id === task)!
  const recommendedProvider = PROVIDERS.find(p => p.id === currentTask.recommendedProvider)

  const finish = () => {
    markWelcomeSeen()
    if (devMode) {
      window.localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
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
          toast.success('Working directory updated')
        } catch (e) {
          console.warn('configure working_dir failed:', e)
          toast.error('Could not save working directory — restart Shannon with --working-dir to apply')
        }
      }
    } catch (e) {
      console.warn('Welcome folder picker failed:', e)
      toast.error('Folder picker failed')
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
      toast.error('Failed to configure provider. You can finish setup in Settings.')
    }
    setSaving(false)
  }

  const toggleTool = (id: string) => {
    setEnabledTools(prev => ({ ...prev, [id]: !prev[id] }))
  }

  return (
    <div className="min-h-screen bg-background text-on-surface flex flex-col">
      <header className="flex items-center justify-between px-xl py-lg">
        <div className="flex items-center gap-sm">
          <span className="material-symbols-outlined text-primary">auto_awesome</span>
          <span className="font-headline-md text-on-surface">Shannon</span>
        </div>
        <button
          onClick={finish}
          className="font-label-md text-on-surface-variant hover:text-primary cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded px-xs"
          aria-label="Skip welcome and start using Shannon"
        >
          Skip →
        </button>
      </header>

      <main className="flex-1 flex items-center justify-center px-xl py-xl">
        <div className="w-full max-w-xl">
          <Stepper step={step} />

          {/* Step 0: Task */}
          {step === 0 && (
            <Card
              title="What will you use Shannon for?"
              subtitle="Pick a starting point. You can change anything later."
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
                    Continue →
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
                        <div className="font-headline-md text-on-surface">{t.label}</div>
                        <div className="font-body-sm text-on-surface-variant mt-xs">{t.blurb}</div>
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
              title="Choose your AI provider"
              subtitle={
                recommendedProvider
                  ? `For ${currentTask.label}, we recommend ${recommendedProvider.label}.`
                  : 'You can change this any time in Settings → Models.'
              }
              footer={
                <>
                  <button onClick={() => setStep(0)} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
                    ← Back
                  </button>
                  <button
                    onClick={handleModelSubmit}
                    disabled={saving || (provider !== 'ollama' && !apiKey)}
                    className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed hover:bg-primary/90 transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                  >
                    {saving ? 'Saving…' : 'Continue →'}
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
                        <div className="font-body-sm text-on-surface-variant">{p.description}</div>
                      </div>
                      <div className={`w-5 h-5 rounded-full border-2 shrink-0 ${provider === p.id ? 'border-primary bg-primary' : 'border-outline-variant'}`} />
                    </div>
                  </button>
                ))}
              </div>
              {provider !== 'ollama' && (
                <div className="mb-lg">
                  <label htmlFor="welcome-api-key" className="font-label-md text-on-surface-variant block mb-xs">API key</label>
                  <input
                    id="welcome-api-key"
                    type="password"
                    value={apiKey}
                    onChange={e => setApiKey(e.target.value)}
                    placeholder="sk-..."
                    autoComplete="off"
                    className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm"
                  />
                  <p className="font-label-sm text-on-surface-variant mt-xs">
                    Stored locally via Shannon's config. Get a key from your provider's dashboard.
                  </p>
                </div>
              )}
            </Card>
          )}

          {/* Step 2: Tools */}
          {step === 2 && (
            <Card
              title="Pick your tools"
              subtitle="Recommended for your task. Toggle off anything you don't need — you can change this in Extensions."
              footer={
                <>
                  <button onClick={() => setStep(1)} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
                    ← Back
                  </button>
                  <button
                    onClick={() => setStep(3)}
                    className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                  >
                    Continue →
                  </button>
                </>
              }
            >
              <div className="space-y-sm mb-lg">
                {Object.entries(TOOL_CATALOG).map(([id, meta]) => {
                  const enabled = !!enabledTools[id]
                  const recommended = currentTask.tools.includes(id)
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
                        aria-label={`Enable ${meta.label}`}
                      />
                      <span className="material-symbols-outlined text-on-surface-variant shrink-0">{meta.icon}</span>
                      <div className="flex-1">
                        <div className="flex items-center gap-xs">
                          <span className="font-headline-md text-on-surface">{meta.label}</span>
                          {recommended && (
                            <span className="text-[10px] uppercase tracking-wider font-bold text-primary bg-primary/10 px-1.5 py-0.5 rounded">
                              Recommended
                            </span>
                          )}
                        </div>
                        <div className="font-body-sm text-on-surface-variant mt-xs">{meta.description}</div>
                      </div>
                    </label>
                  )
                })}
              </div>
              <p className="font-body-sm text-on-surface-variant">
                Need a different working directory? Adjust it in{' '}
                <button onClick={() => { markWelcomeSeen(); navigate('/settings/general') }} className="text-primary hover:underline cursor-pointer">
                  Settings → General
                </button>
                .
              </p>
            </Card>
          )}

          {/* Step 3: Done */}
          {step === 3 && (
            <Card
              title="You're all set"
              subtitle="Here are the keys to know. You can change anything in Settings."
              footer={
                <>
                  <button onClick={() => setStep(2)} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
                    ← Back
                  </button>
                  <button
                    onClick={finish}
                    className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                  >
                    Start using Shannon →
                  </button>
                </>
              }
            >
              {/* Summary */}
              <div className="bg-surface-container-low rounded-xl p-md mb-md">
                <div className="font-label-sm text-on-surface-variant mb-xs">Your setup</div>
                <ul className="space-y-xs text-body-sm text-on-surface">
                  <li className="flex items-center gap-sm">
                    <span className="material-symbols-outlined text-[18px] text-primary">{currentTask.icon}</span>
                    <span>{currentTask.label}</span>
                  </li>
                  <li className="flex items-center gap-sm">
                    <span className="material-symbols-outlined text-[18px] text-primary">memory</span>
                    <span>{PROVIDERS.find(p => p.id === provider)?.label ?? provider}</span>
                  </li>
                  <li className="flex items-center gap-sm">
                    <span className="material-symbols-outlined text-[18px] text-primary">build</span>
                    <span>
                      {Object.entries(enabledTools).filter(([, on]) => on).length} tool{Object.entries(enabledTools).filter(([, on]) => on).length === 1 ? '' : 's'} enabled
                    </span>
                  </li>
                </ul>
              </div>

              {/* Optional workspace picker */}
              <div className="bg-surface-container-low rounded-xl p-md mb-md">
                <div className="font-label-sm text-on-surface-variant mb-xs">Working directory</div>
                <div className="font-mono text-on-surface text-sm break-all mb-sm">{pickedDir ?? config?.working_dir ?? '(defaults to home dir)'}</div>
                <button
                  onClick={pickDirectory}
                  className="px-md py-sm bg-surface-container-low hover:bg-surface-container-high border border-outline-variant/50 rounded-lg font-label-md text-on-surface cursor-pointer transition-colors flex items-center gap-sm focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                >
                  <span className="material-symbols-outlined text-[18px]">folder_open</span>
                  {pickedDir ? 'Choose a different folder' : 'Choose folder'}
                </button>
              </div>

              {/* Shortcuts */}
              <div className="space-y-sm">
                <div className="font-label-md text-on-surface-variant mb-xs">Shortcuts</div>
                {TOP_SHORTCUTS.map(s => (
                  <div key={s.keys} className="flex items-center justify-between py-xs">
                    <span className="font-body-sm text-on-surface-variant">{s.action}</span>
                    <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono shrink-0">{s.keys}</kbd>
                  </div>
                ))}
              </div>
              <p className="font-body-sm text-on-surface-variant mt-md">
                Press <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono">?</kbd> any time for the full list.
              </p>

              {/* Developer mode opt-in */}
              <label className="mt-md flex items-start gap-sm p-md rounded-xl border border-outline-variant/50 hover:border-primary/50 cursor-pointer transition-all">
                <input
                  type="checkbox"
                  checked={devMode}
                  onChange={() => setDevMode(v => !v)}
                  className="mt-xs accent-primary"
                  aria-label="Enable developer features"
                />
                <div>
                  <div className="font-headline-md text-on-surface">I'm a developer — show advanced features</div>
                  <div className="font-body-sm text-on-surface-variant mt-xs">
                    Reveals the Schedules, Triggers, Permission Modes, and Extensions sections in the sidebar. You can toggle this later.
                  </div>
                </div>
              </label>
            </Card>
          )}
        </div>
      </main>
    </div>
  )
}

function Stepper({ step }: { step: number }) {
  return (
    <div className="flex items-center justify-center gap-sm mb-xl" aria-label={`Step ${step + 1} of ${STEPS.length}: ${STEPS[step]}`}>
      {STEPS.map((label, i) => (
        <div key={label} className="flex items-center gap-sm">
          <div className={`w-2 h-2 rounded-full ${i <= step ? 'bg-primary' : 'bg-outline-variant'}`} />
          {i === step && <span className="font-label-sm text-primary">{label}</span>}
          {i < STEPS.length - 1 && <div className="w-8 h-px bg-outline-variant" aria-hidden="true" />}
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
