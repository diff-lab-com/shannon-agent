import { useId, useState } from 'react'
import { Combobox } from '@base-ui/react/combobox'
import { cn } from '@/lib/utils'

export interface ComboboxOption {
  value: string
  label: string
}

interface ComboboxSelectProps {
  options: ComboboxOption[]
  value: string | null
  onChange: (value: string | null) => void
  placeholder?: string
  label?: string
  emptyText?: string
  className?: string
  disabled?: boolean
}

/**
 * Searchable select (Base UI Combobox, token-styled copy-in). For ≤8 static
 * options the plain Select is fine; use this when the list is long or the
 * user needs type-to-filter (model pickers, connector catalogs, path pickers).
 */
export function ComboboxSelect({ options, value, onChange, placeholder, label, emptyText = 'No matches', className, disabled }: ComboboxSelectProps) {
  const id = useId()
  const [inputValue, setInputValue] = useState('')
  const selected = options.find(o => o.value === value)

  return (
    <div className={cn('flex flex-col gap-xs', className)}>
      {label && <label id={`${id}-label`} className="font-label-md text-on-surface">{label}</label>}
      <Combobox.Root
        value={selected ?? null}
        onValueChange={(v) => { const sel = v as unknown as ComboboxOption | null; onChange(sel?.value ?? null) }}
        inputValue={inputValue}
        onInputValueChange={setInputValue}
        disabled={disabled}
        items={options}
      >
        <Combobox.Trigger
          aria-labelledby={label ? `${id}-label` : undefined}
          className={cn(
            'flex w-full items-center justify-between gap-sm rounded-lg border border-outline-variant/50 bg-surface-container-lowest px-sm py-xs',
            'font-body-md text-on-surface hover:border-primary/50 data-[popup-open]:border-primary',
            'focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary disabled:opacity-50',
          )}
        >
          <span className={cn('truncate', !selected && 'text-on-surface-variant/60')}>
            {selected?.label ?? placeholder}
          </span>
          <Combobox.Icon className="text-on-surface-variant">
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">expand_more</span>
          </Combobox.Icon>
        </Combobox.Trigger>
        <Combobox.Portal>
          <Combobox.Positioner sideOffset={4} className="z-modal outline-none">
            <Combobox.Popup className="w-[var(--anchor-width)] max-h-72 overflow-y-auto rounded-xl border border-outline-variant/40 bg-surface-container-lowest shadow-e3 [backdrop-filter:var(--glass-blur-overlay)]">
              <Combobox.Input
                placeholder={placeholder}
                className="w-full border-b border-outline-variant/30 bg-transparent px-sm py-xs font-body-md text-on-surface outline-none placeholder:text-on-surface-variant/60"
              />
              <Combobox.List>
                {options.map(opt => (
                  <Combobox.Item
                    key={opt.value}
                    value={opt}
                    className={cn(
                      'flex cursor-pointer items-center justify-between px-sm py-xs font-label-md text-on-surface',
                      'data-[highlighted]:bg-primary/10 data-[highlighted]:text-primary',
                      'data-[selected]:font-bold',
                    )}
                  >
                    <span className="truncate">{opt.label}</span>
                    {opt.value === value && (
                      <span className="material-symbols-outlined text-[16px] text-primary" aria-hidden="true">check</span>
                    )}
                  </Combobox.Item>
                ))}
                <Combobox.Empty>
                  <span className="block px-sm py-md text-center font-body-sm text-on-surface-variant">{emptyText}</span>
                </Combobox.Empty>
              </Combobox.List>
            </Combobox.Popup>
          </Combobox.Positioner>
        </Combobox.Portal>
      </Combobox.Root>
    </div>
  )
}
