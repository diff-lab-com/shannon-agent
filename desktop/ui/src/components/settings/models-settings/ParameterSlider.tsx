import { useEffect, useId, useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'

export function ParameterSlider({ label, value, min, max, step, formatValue, lowLabel, highLabel, configKey }: {
  label: string
  value: number
  min: number
  max: number
  step: number
  formatValue?: (v: number) => string
  lowLabel?: string
  highLabel?: string
  configKey?: string
}) {
  const intl = useIntl()
  const [local, setLocal] = useState(value)
  const inputId = useId()
  // Keep the slider in sync with the persisted config value so it reflects
  // reality (initial load + external updates) instead of a stale literal.
  useEffect(() => { setLocal(value) }, [value])
  const display = formatValue ? formatValue(local) : String(local)

  // P1-11: dragging used to fire one IPC write per notch. Track the last
  // value handed to the backend so a release/blur/keyup burst dedupes to a
  // single write of the final position.
  const lastCommitted = useRef<number | null>(value)

  const commit = (newValue: number) => {
    if (!configKey || lastCommitted.current === newValue) return
    lastCommitted.current = newValue
    api.configure({ key: configKey, value: String(newValue) }).catch(e => {
      // Persist failed — put the slider back on the persisted value instead
      // of silently diverging from disk.
      toastError(intl.formatMessage({ id: 'settings.models.paramSaveFailed' }, { key: configKey }), e)
      lastCommitted.current = value
      setLocal(value)
    })
  }

  // Change only moves the thumb; the write happens once the gesture ends
  // (pointer release, keyboard step, or blur as the catch-all).
  const handleChange = (newValue: number) => {
    setLocal(newValue)
  }

  const handleCommit = () => {
    commit(local)
  }

  return (
    <div>
      <div className="flex justify-between items-center mb-sm">
        <label htmlFor={inputId} className="font-label-md text-on-surface-variant">{label}</label>
        <span className="font-label-sm text-primary bg-primary-container/20 px-sm py-xs rounded">{display}</span>
      </div>
      <input
        id={inputId}
        className="w-full appearance-none bg-outline-variant/30 h-1 rounded-full cursor-pointer outline-none slider-thumb-primary"
        min={min} max={max} step={step} type="range" value={local}
        onChange={e => handleChange(Number(e.target.value))}
        onPointerUp={handleCommit}
        onKeyUp={handleCommit}
        onBlur={handleCommit}
      />
      {lowLabel && highLabel ? (
        <div className="flex justify-between mt-xs">
          <span className="font-label-sm text-on-surface-variant opacity-50">{lowLabel}</span>
          <span className="font-label-sm text-on-surface-variant opacity-50">{highLabel}</span>
        </div>
      ) : null}
    </div>
  )
}
