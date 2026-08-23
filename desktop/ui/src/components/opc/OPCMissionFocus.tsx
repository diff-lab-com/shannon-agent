import { useState, useEffect } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'

interface Props {
  config: { provider?: string; strategic_focus?: string } | null
}

export default function OPCMissionFocus({ config }: Props) {
  const intl = useIntl()
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState('')

  const focus = config?.strategic_focus
    || (config?.provider
      ? `${config.provider.charAt(0).toUpperCase() + config.provider.slice(1)} Agent Orchestration — autonomous task execution with multi-agent coordination.`
      : intl.formatMessage({ id: 'opc.missionFocus.defaultFocus' }))

  useEffect(() => { setText(focus) }, [focus])

  const save = () => {
    api.configure({ key: 'strategic_focus', value: text })
      .then(() => toast.success(intl.formatMessage({ id: 'opc.missionFocus.focusSaved' })))
      .catch((e) => toastError(intl.formatMessage({ id: 'opc.missionFocus.saveFailed' }), e))
    setEditing(false)
  }

  return (
    <div className="bg-surface-container-lowest/70 backdrop-blur-md rounded-2xl p-xl mb-lg border border-outline-variant/30 relative shadow-sm">
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2 uppercase font-label-md text-[13px] tracking-widest text-on-surface-variant font-bold">
          <span className="w-1.5 h-1.5 bg-outline-variant rotate-45 block" />
          {intl.formatMessage({ id: 'opc.missionFocus.todayMission' })}
        </div>
        <Button
          variant="link"
          size="sm"
          className="text-label-sm h-auto px-0 text-primary hover:underline"
          onClick={() => setEditing(!editing)}
          aria-expanded={editing}
        >
          {editing ? intl.formatMessage({ id: 'opc.missionFocus.cancel' }) : intl.formatMessage({ id: 'opc.missionFocus.edit' })}
        </Button>
      </div>
      {editing ? (
        <div className="mt-2 space-y-md">
          <textarea
            className="w-full h-24 p-md bg-surface-container-low rounded-xl border border-outline-variant/30 text-body-md resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
            value={text}
            onChange={e => setText(e.target.value)}
            aria-label={intl.formatMessage({ id: 'opc.missionFocus.editMission.aria' })}
          />
          <Button
            className="px-md py-sm rounded-lg font-label-md hover:opacity-90"
            onClick={save}
          >
            {intl.formatMessage({ id: 'opc.missionFocus.saveFocus' })}
          </Button>
        </div>
      ) : (
        <h2 className="font-headline-lg text-[28px] font-bold text-on-surface mt-2 max-w-5xl">
          {focus}
        </h2>
      )}
    </div>
  )
}
