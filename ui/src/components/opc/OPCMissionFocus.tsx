import { useState, useEffect } from 'react'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'

interface Props {
  config: { provider?: string; strategic_focus?: string } | null
}

export default function OPCMissionFocus({ config }: Props) {
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState('')

  const focus = config?.strategic_focus
    || (config?.provider
      ? `${config.provider.charAt(0).toUpperCase() + config.provider.slice(1)} Agent Orchestration — autonomous task execution with multi-agent coordination.`
      : 'Autonomous task execution through multi-agent orchestration and intelligent coordination.')

  useEffect(() => { setText(focus) }, [focus])

  const save = () => {
    api.configure({ key: 'strategic_focus', value: text })
      .then(() => toast.success('Strategic focus saved'))
      .catch(() => toast.error('Failed to save focus'))
    setEditing(false)
  }

  return (
    <div className="bg-surface-container-lowest/70 backdrop-blur-md rounded-2xl p-xl mb-lg border border-outline-variant/30 relative shadow-sm">
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2 uppercase font-label-md text-[13px] tracking-widest text-on-surface-variant font-bold">
          <span className="w-1.5 h-1.5 bg-outline-variant rotate-45 block" />
          Strategic Focus
        </div>
        <button
          className="text-label-sm text-primary hover:underline cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
          onClick={() => setEditing(!editing)}
          aria-expanded={editing}
        >
          {editing ? 'Cancel' : 'Edit'}
        </button>
      </div>
      {editing ? (
        <div className="mt-2 space-y-md">
          <textarea
            className="w-full h-24 p-md bg-surface-container-low rounded-xl border border-outline-variant/30 text-body-md resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
            value={text}
            onChange={e => setText(e.target.value)}
            aria-label="Edit strategic focus"
          />
          <button
            className="px-md py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:opacity-90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
            onClick={save}
          >
            Save Focus
          </button>
        </div>
      ) : (
        <h2 className="font-headline-lg text-[28px] font-bold text-on-surface mt-2 max-w-5xl">
          {focus}
        </h2>
      )}
    </div>
  )
}
