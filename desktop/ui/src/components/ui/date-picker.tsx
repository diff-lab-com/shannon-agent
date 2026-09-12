import { useId } from 'react'
import { cn } from '@/lib/utils'

export interface DatePickerProps {
  value: string | null
  onChange: (isoDate: string | null) => void
  label?: string
  min?: string
  max?: string
  required?: boolean
  className?: string
  id?: string
}

/**
 * Date picker built on the native date input — zero-dependency, gets the
 * platform calendar UI for free (Electron/Chromium), and keyboard/screen-
 * reader behaviour matches the OS. Styled to the theme tokens; the glass /
 * density work stays in CSS like every other primitive here. A custom
 * calendar popover (react-day-picker) is only worth it if we need range
 * selection or blocked-out dates — revisit when a screen needs it.
 */
export function DatePicker({ value, onChange, label, min, max, required, className, id: idProp }: DatePickerProps) {
  const autoId = useId()
  const id = idProp ?? autoId
  return (
    <div className={cn('flex flex-col gap-xs', className)}>
      {label && (
        <label htmlFor={id} className="font-label-md text-on-surface">
          {label}
          {required && <span aria-hidden="true" className="text-error ml-0.5">*</span>}
        </label>
      )}
      <input
        id={id}
        type="date"
        value={value ?? ''}
        min={min}
        max={max}
        required={required}
        onChange={e => onChange(e.target.value || null)}
        className={cn(
          'rounded-lg border border-outline-variant/50 bg-surface-container-lowest px-sm py-xs font-body-md text-on-surface',
          'focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
          '[color-scheme:light] dark:[color-scheme:dark]',
        )}
      />
    </div>
  )
}
