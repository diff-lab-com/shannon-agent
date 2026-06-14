import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import { useApp } from '@/context/AppContext'

const PROVIDERS = [
  { id: 'anthropic', label: 'Anthropic', description: 'Claude Sonnet 4.6 — recommended for coding' },
  { id: 'openai', label: 'OpenAI', description: 'GPT-4o / o1 — strong reasoning, multi-modal' },
  { id: 'ollama', label: 'Ollama', description: 'Local models, no API key needed' },
  { id: 'deepseek', label: 'DeepSeek', description: 'Cost-effective, long-context tasks' },
] as const

const TOP_SHORTCUTS = [
  { keys: '⌘ K', action: 'Open command palette' },
  { keys: '⌘ N', action: 'New chat' },
  { keys: '⌘ 1 / 2 / 3', action: 'Jump to Chat / Goals / Tasks' },
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

const STEPS = ['Provider', 'Workspace', 'Shortcuts'] as const

export default function Welcome() {
  const navigate = useNavigate()
  const { refreshConfig, refreshStatus, config } = useApp()
  const [step, setStep] = useState(0)
  const [provider, setProvider] = useState<string>('anthropic')
  const [apiKey, setApiKey] = useState('')
  const [saving, setSaving] = useState(false)

  const finish = () => {
    markWelcomeSeen()
    navigate('/chat', { replace: true })
  }

  const handleProviderSubmit = async () => {
    setSaving(true)
    try {
      if (provider !== 'ollama' && apiKey) {
        await api.configure({ key: 'api_key', value: apiKey })
      }
      await api.switchProvider({ provider, model: '' }).catch(e => console.warn('switchProvider in welcome:', e))
      await Promise.all([refreshConfig(), refreshStatus()])
      setStep(1)
    } catch (e) {
      console.warn('Welcome provider setup failed:', e)
      toast.error('Failed to configure provider. You can finish setup in Settings.')
    }
    setSaving(false)
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

          {step === 0 && (
            <Card
              title="Choose your AI provider"
              subtitle="You can change this any time in Settings → Models."
              footer={
                <>
                  <span />
                  <button
                    onClick={handleProviderSubmit}
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

          {step === 1 && (
            <Card
              title="Workspace"
              subtitle="Shannon will read and modify files in this directory."
              footer={
                <>
                  <button onClick={() => setStep(0)} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
                    ← Back
                  </button>
                  <button
                    onClick={() => setStep(2)}
                    className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                  >
                    Continue →
                  </button>
                </>
              }
            >
              <div className="bg-surface-container-low rounded-xl p-md mb-md">
                <div className="font-label-sm text-on-surface-variant mb-xs">Current working directory</div>
                <div className="font-mono text-on-surface text-sm break-all">{config?.working_dir ?? '(not set — defaults to home dir)'}</div>
              </div>
              <p className="font-body-sm text-on-surface-variant mb-md">
                To work in a specific project, restart Shannon from that folder, or launch with{' '}
                <code className="font-mono bg-surface-container-high px-xs rounded text-[12px]">--working-dir &lt;path&gt;</code>.
              </p>
              <p className="font-body-sm text-on-surface-variant">
                Prefer a different autonomy level? Adjust it in{' '}
                <button onClick={() => { markWelcomeSeen(); navigate('/settings/general') }} className="text-primary hover:underline cursor-pointer">
                  Settings → General
                </button>{' '}
                (Suggest / Plan / Auto Edit / Full Auto).
              </p>
            </Card>
          )}

          {step === 2 && (
            <Card
              title="Shortcuts"
              subtitle="The most important keys to know."
              footer={
                <>
                  <button onClick={() => setStep(1)} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
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
              <div className="space-y-sm mb-lg">
                {TOP_SHORTCUTS.map(s => (
                  <div key={s.keys} className="flex items-center justify-between py-xs">
                    <span className="font-body-sm text-on-surface-variant">{s.action}</span>
                    <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono shrink-0">{s.keys}</kbd>
                  </div>
                ))}
              </div>
              <p className="font-body-sm text-on-surface-variant">
                Press <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono">?</kbd> any time for the full list.
              </p>
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
