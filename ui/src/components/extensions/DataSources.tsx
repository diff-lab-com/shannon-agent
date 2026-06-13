import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/ui/empty-state'
import { Input } from '@/components/ui/input'
import { useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import type { McpServerInfo } from '@/types'

export default function DataSources() {
  const { mcpServers, refreshMcpServers } = useApp()
  const [adding, setAdding] = useState(false)
  const [newName, setNewName] = useState('')
  const [newCommand, setNewCommand] = useState('')
  const [newArgs, setNewArgs] = useState('')
  const [restarting, setRestarting] = useState<string | null>(null)
  const [removeTarget, setRemoveTarget] = useState<string | null>(null)
  const [validationErrors, setValidationErrors] = useState<{ name?: string; command?: string }>({})

  const handleAdd = async () => {
    const errors: { name?: string; command?: string } = {}
    if (!newName.trim()) errors.name = 'Name is required'
    if (!newCommand.trim()) errors.command = 'Command is required'
    if (Object.keys(errors).length > 0) { setValidationErrors(errors); return }
    setValidationErrors({})
    try {
      await api.addMcpServer(newName.trim(), newCommand.trim(), newArgs.trim() ? newArgs.trim().split(/\s+/) : [], {})
      setNewName('')
      setNewCommand('')
      setNewArgs('')
      setAdding(false)
      await refreshMcpServers()
      toast.success(`Added ${newName.trim()}`)
    } catch (e) { console.warn("DataSources error:", e); toast.error('Failed to add server') }
  }

  const handleRemove = async (name: string) => {
    try {
      await api.removeMcpServer(name)
      await refreshMcpServers()
      toast.success(`Removed ${name}`)
    } catch (e) { console.warn("DataSources error:", e); toast.error('Failed to remove server') }
    setRemoveTarget(null)
  }

  const handleRestart = async (name: string) => {
    setRestarting(name)
    try { await api.restartMcpServer(name); toast.success(`Restarted ${name}`) } catch (e) { console.warn("DataSources error:", e); toast.error(`Failed to restart ${name}`) }
    setRestarting(null)
    await refreshMcpServers()
  }

  return (
    <div className="max-w-[1200px] mx-auto px-lg py-xl">
      <div className="mb-lg flex items-center justify-between">
        <div>
          <h2 className="text-headline-lg font-headline-lg text-on-surface">Data Sources</h2>
          <p className="text-body-md text-on-surface-variant max-w-2xl">Manage MCP servers that provide tools and data to your agents.</p>
        </div>
        <Button
          className="px-lg py-sm bg-primary text-on-primary rounded-xl font-bold flex items-center gap-sm hover:shadow-md active:scale-95 transition-all cursor-pointer"
          onClick={() => setAdding(true)}
        >
          <span className="material-symbols-outlined text-[20px]">add</span>
          Add Source
        </Button>
      </div>

      {/* Add Form */}
      {adding && (
        <div className="mb-lg bg-surface-container-lowest border border-primary/30 rounded-xl p-lg shadow-sm">
          <h3 className="font-headline-md text-on-surface mb-md">Add MCP Server</h3>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-md mb-md">
            <div>
              <Input className={`bg-surface-container-lowest border rounded-lg px-md py-sm font-body-sm ${validationErrors.name ? 'border-error' : 'border-outline-variant/50'}`} placeholder="Name (e.g. my-server)" value={newName} onChange={e => { setNewName(e.target.value); setValidationErrors({}) }} />
              {validationErrors.name ? <p className="text-error text-label-sm mt-xs">{validationErrors.name}</p> : null}
            </div>
            <div>
              <Input className={`bg-surface-container-lowest border rounded-lg px-md py-sm font-body-sm ${validationErrors.command ? 'border-error' : 'border-outline-variant/50'}`} placeholder="Command (e.g. npx my-mcp-server)" value={newCommand} onChange={e => { setNewCommand(e.target.value); setValidationErrors({}) }} />
              {validationErrors.command ? <p className="text-error text-label-sm mt-xs">{validationErrors.command}</p> : null}
            </div>
            <Input className="bg-surface-container-lowest border border-outline-variant/50 rounded-lg px-md py-sm font-body-sm" placeholder="Args (space-separated, optional)" value={newArgs} onChange={e => setNewArgs(e.target.value)} />
          </div>
          <div className="flex gap-sm">
            <Button className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer" onClick={handleAdd}>Add Server</Button>
            <Button className="px-lg py-sm border border-outline-variant rounded-lg font-label-md text-on-surface cursor-pointer" onClick={() => { setAdding(false); setValidationErrors({}) }}>Cancel</Button>
          </div>
        </div>
      )}

      <div className="grid grid-cols-12 gap-lg pb-10">
        {mcpServers.length === 0 && !adding && (
          <div className="col-span-12">
            <EmptyState
              icon="cloud_off"
              title="No MCP servers configured."
              description='Click "Add Source" to connect a data source.'
            />
          </div>
        )}

        {mcpServers.map(server => (
          <McpServerCard
            key={server.name}
            server={server}
            restarting={restarting}
            onRestart={handleRestart}
            onRemove={setRemoveTarget}
          />
        ))}

        {/* Add New Source Card */}
        {!adding && (
          <div className="col-span-12 lg:col-span-4 bg-surface-container-low/50 border border-dashed border-outline-variant rounded-xl p-md flex flex-col justify-center items-center gap-md min-h-[140px] group hover:border-primary/50 transition-colors cursor-pointer" onClick={() => setAdding(true)}>
            <p className="font-label-md text-label-md font-medium text-on-surface-variant">Add New Source</p>
            <div className="w-12 h-12 rounded-full bg-surface-container border border-outline-variant border-dashed flex items-center justify-center text-on-surface-variant group-hover:bg-primary-container/20 group-hover:text-primary transition-all">
              <span className="material-symbols-outlined text-[28px]">add</span>
            </div>
          </div>
        )}
      </div>

      {/* Remove Confirmation Modal */}
      {removeTarget && (
        <div className="fixed inset-0 z-50 bg-black/30 backdrop-blur-sm flex items-center justify-center" onClick={() => setRemoveTarget(null)} onKeyDown={e => { if (e.key === 'Escape') setRemoveTarget(null) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl shadow-xl border border-outline-variant/30 max-w-sm w-full mx-md" onClick={e => e.stopPropagation()}>
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-error text-[24px]">delete</span>
              <h3 className="font-headline-md text-on-surface">Remove Data Source</h3>
            </div>
            <p className="text-body-md text-on-surface-variant mb-lg">Are you sure you want to remove <strong className="text-on-surface">{removeTarget}</strong>? Any agents using its tools will lose access.</p>
            <div className="flex justify-end gap-sm">
              <Button className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container" onClick={() => setRemoveTarget(null)}>Cancel</Button>
              <Button className="px-lg py-sm rounded-xl bg-error text-on-error hover:bg-error/90" onClick={() => handleRemove(removeTarget)}>Remove</Button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

function McpServerCard({ server, restarting, onRestart, onRemove }: {
  server: McpServerInfo
  restarting: string | null
  onRestart: (name: string) => void
  onRemove: (name: string) => void
}) {
  const statusColor = server.connected ? 'bg-tertiary' : 'bg-error'
  const statusText = server.connected ? 'Connected' : 'Disconnected'
  const statusBg = server.connected ? 'bg-tertiary/10 text-tertiary border-tertiary/20' : 'bg-error/10 text-error border-error/20'

  return (
    <div className={`col-span-12 md:col-span-6 lg:col-span-4 bg-surface-container-lowest border rounded-xl p-md shadow-sm hover:shadow-md transition-shadow ${server.connected ? 'border-outline-variant/50' : 'border-error/20'}`}>
      <div className="flex items-center justify-between mb-md">
        <div className="flex items-center gap-md">
          <div className={`w-10 h-10 rounded-lg flex items-center justify-center ${server.connected ? 'bg-primary/10 text-primary' : 'bg-error/10 text-error'}`}>
            <span className="material-symbols-outlined">database</span>
          </div>
          <div>
            <h4 className="font-label-md text-label-md font-bold text-on-surface">{server.name}</h4>
            <p className="text-label-sm font-label-sm text-on-surface-variant">{server.tool_count} tools</p>
          </div>
        </div>
        <span className={`px-sm py-[2px] rounded-full text-label-sm font-label-sm flex items-center gap-xs border ${statusBg}`}>
          <span className={`w-2 h-2 rounded-full ${statusColor} ${server.connected ? '' : ''}`} />
          {statusText}
        </span>
      </div>
      <div className="flex items-center gap-sm pt-sm border-t border-outline-variant/30">
        <Button
          variant="ghost"
          className="flex-1 py-xs rounded-lg font-label-sm text-on-surface-variant hover:text-primary cursor-pointer"
          onClick={() => onRestart(server.name)}
          disabled={restarting === server.name}
        >
          {restarting === server.name ? (
            <span className="material-symbols-outlined animate-spin text-[16px]">progress_activity</span>
          ) : (
            <span className="material-symbols-outlined text-[16px]">sync</span>
          )}
        </Button>
        <Button variant="ghost" aria-label="Remove server" className="py-xs rounded-lg text-on-surface-variant hover:text-error cursor-pointer" onClick={() => onRemove(server.name)}>
          <span className="material-symbols-outlined text-[16px]">delete</span>
        </Button>
      </div>
    </div>
  )
}
