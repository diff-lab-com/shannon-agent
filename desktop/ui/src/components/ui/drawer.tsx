import * as React from 'react'

export interface DrawerProps {
  open?: boolean
  onOpenChange?(open: boolean): void
  children?: React.ReactNode
}

export const Drawer = ({ open, onOpenChange, children }: DrawerProps) => {
  if (!open) return null
  return (
    <div className="fixed inset-0 z-50 flex">
      <div className="fixed inset-0 bg-black/50" onClick={() => onOpenChange?.(false)} />
      <div className="relative ml-auto h-full w-full max-w-md bg-surface-container-low p-6 shadow-xl">
        {children}
      </div>
    </div>
  )
}

export const DrawerContent = ({ children }: { children?: React.ReactNode }) => {
  return <div className="flex flex-col gap-4">{children}</div>
}

export const DrawerHeader = ({ children }: { children?: React.ReactNode }) => {
  return <div className="mb-4">{children}</div>
}

export const DrawerTitle = ({ children }: { children?: React.ReactNode }) => {
  return <h2 className="text-lg font-bold">{children}</h2>
}
