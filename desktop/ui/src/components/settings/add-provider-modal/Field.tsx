// Add/Edit Provider modal — labeled form field helper. Extracted from
// AddProviderModal.tsx (T3.1).
import type { ReactNode } from 'react'

interface FieldProps {
  label: string
  children: ReactNode
}

export function Field({ label, children }: FieldProps) {
  return (
    <label className="block">
      <span className="block font-label-sm text-on-surface-variant mb-xs">{label}</span>
      {children}
    </label>
  )
}
