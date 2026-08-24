// Memory panel — labeled form field helper for MemoryEditor. Extracted
// from MemoryPanel.tsx (T3.1).
import type { ReactNode } from 'react'

interface FieldProps {
  label: string
  children: ReactNode
}

export function Field({ label, children }: FieldProps) {
  return (
    <label className="block">
      <span className="block text-label-sm text-on-surface-variant mb-xs">{label}</span>
      {children}
    </label>
  )
}
