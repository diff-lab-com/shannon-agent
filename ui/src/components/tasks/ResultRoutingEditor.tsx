// ResultRoutingEditor — Phase D P2.3.
//
// Selects where scheduled-routine results get delivered. Four channel
// presets (slack / email / notification / log); each entry is a free-form
// target spec stored as a string in ExecutionPolicy.result_routing. The
// backend dispatches each entry to its adapter when a run finishes.
//
// Wire format mirrors what the eventual adapters will parse:
//   slack:#channel      → Slack channel mention
//   email:user@domain   → email recipient
//   notification        → in-app/desktop notification (no args)
//   log                 → append to routine log file (no args)
//
// Empty list = log only (the pre-P2.3 default).

import { useState } from 'react'

export type RoutingKind = 'slack' | 'email' | 'notification' | 'log'

interface ResultRoutingEditorProps {
  value: string[]
  onChange: (next: string[]) => void
}

const KIND_OPTIONS: { kind: RoutingKind; icon: string; label: string; placeholder: string }[] = [
  { kind: 'slack', icon: 'tag', label: 'Slack channel', placeholder: '#ops' },
  { kind: 'email', icon: 'mail', label: 'Email', placeholder: 'user@example.com' },
  { kind: 'notification', icon: 'notifications', label: 'Notification', placeholder: '' },
  { kind: 'log', icon: 'description', label: 'Log file', placeholder: '' },
]

export function encodeChannel(kind: RoutingKind, target: string): string {
  if (kind === 'notification' || kind === 'log') return kind
  const trimmed = target.trim()
  return trimmed ? `${kind}:${trimmed}` : ''
}

export function parseChannel(entry: string): { kind: RoutingKind; target: string } | null {
  if (entry === 'notification') return { kind: 'notification', target: '' }
  if (entry === 'log') return { kind: 'log', target: '' }
  const colon = entry.indexOf(':')
  if (colon < 0) return null
  const rawKind = entry.slice(0, colon)
  if (rawKind === 'slack' || rawKind === 'email') {
    return { kind: rawKind, target: entry.slice(colon + 1) }
  }
  return null
}

export default function ResultRoutingEditor({ value, onChange }: ResultRoutingEditorProps) {
  const [pendingKind, setPendingKind] = useState<RoutingKind>('slack')
  const [pendingTarget, setPendingTarget] = useState('')

  const addChannel = () => {
    const encoded = encodeChannel(pendingKind, pendingTarget)
    if (!encoded) return
    if (value.includes(encoded)) return
    onChange([...value, encoded])
    setPendingTarget('')
  }

  const removeChannel = (entry: string) => {
    onChange(value.filter(v => v !== entry))
  }

  return (
    <div className="flex flex-col gap-sm">
      <div className="font-label-md text-on-surface-variant">Result routing</div>
      <div className="font-label-sm text-[11px] text-on-surface-variant">
        Where to send results when this routine finishes. Empty = log only.
      </div>

      {value.length > 0 ? (
        <ul className="flex flex-col gap-xs" aria-label="Configured channels">
          {value.map(entry => {
            const parsed = parseChannel(entry)
            const kind = parsed?.kind ?? 'log'
            const target = parsed?.target ?? ''
            const opt = KIND_OPTIONS.find(o => o.kind === kind)
            return (
              <li
                key={entry}
                className="flex items-center gap-sm bg-surface-container-low/60 rounded-md px-sm py-xs border border-outline-variant/20"
              >
                <span className="material-symbols-outlined text-[14px] text-on-surface-variant">{opt?.icon ?? 'circle'}</span>
                <span className="font-label-md text-on-surface flex-1 truncate">
                  {kind === 'slack' || kind === 'email' ? `${kind}: ${target}` : opt?.label ?? kind}
                </span>
                <button
                  type="button"
                  aria-label={`Remove ${entry}`}
                  className="text-on-surface-variant hover:text-error cursor-pointer p-xs rounded focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                  onClick={() => removeChannel(entry)}
                >
                  <span className="material-symbols-outlined text-[14px]">close</span>
                </button>
              </li>
            )
          })}
        </ul>
      ) : (
        <p className="font-label-sm text-[11px] text-on-surface-variant italic">No channels configured.</p>
      )}

      <div className="flex flex-col md:flex-row gap-xs">
        <label className="flex items-center gap-xs">
          <span className="sr-only">Channel type</span>
          <select
            aria-label="Channel type"
            value={pendingKind}
            onChange={e => { setPendingKind(e.target.value as RoutingKind); setPendingTarget('') }}
            className="bg-surface-container-low rounded-md border border-outline-variant/30 px-sm py-xs font-label-md focus:outline-none focus:ring-2 focus:ring-primary/30"
          >
            {KIND_OPTIONS.map(o => <option key={o.kind} value={o.kind}>{o.label}</option>)}
          </select>
        </label>
        {pendingKind === 'slack' || pendingKind === 'email' ? (
          <input
            type="text"
            aria-label={`${pendingKind} target`}
            placeholder={KIND_OPTIONS.find(o => o.kind === pendingKind)?.placeholder}
            value={pendingTarget}
            onChange={e => setPendingTarget(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); addChannel() } }}
            className="flex-1 bg-surface-container-low rounded-md border border-outline-variant/30 px-sm py-xs font-label-md focus:outline-none focus:ring-2 focus:ring-primary/30"
          />
        ) : null}
        <button
          type="button"
          className="px-md py-xs bg-primary text-on-primary rounded-md font-label-md cursor-pointer disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          onClick={addChannel}
          disabled={pendingKind !== 'notification' && pendingKind !== 'log' && !pendingTarget.trim()}
        >
          Add
        </button>
      </div>
    </div>
  )
}
