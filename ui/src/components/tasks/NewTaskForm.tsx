// Inline form for creating a new background task (prompt-based, fires immediately).
//
// Preserves the original monolith's behavior: Enter submits (Shift+Enter for
// newline), char counter, Create Task disabled when empty.

import { Button } from '@/components/ui/button'

interface NewTaskFormProps {
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  onCancel: () => void
}

export default function NewTaskForm({ value, onChange, onSubmit, onCancel }: NewTaskFormProps) {
  return (
    <div className="bg-surface-container-lowest border border-primary/30 rounded-xl p-lg mb-lg flex flex-col gap-md shadow-sm">
      <h3 className="font-body-lg font-bold text-on-surface">Create Background Task</h3>
      <textarea
        className={`w-full h-20 p-sm bg-surface-container-low rounded-lg border text-body-sm resize-none focus:outline-none focus:ring-2 focus:ring-primary/30 ${!value.trim() ? 'border-outline-variant/30' : 'border-primary/30'}`}
        placeholder="Describe the task for background execution..."
        value={value}
        onChange={e => onChange(e.target.value)}
        onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey && value.trim()) { e.preventDefault(); onSubmit() } }}
        autoFocus
      />
      <div className="flex items-center justify-between">
        <span className="font-label-sm text-on-surface-variant">{value.length > 0 ? `${value.length} chars` : ''}</span>
        <div className="flex gap-sm">
          <Button
            className="px-md py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50"
            onClick={onSubmit}
            disabled={!value.trim()}
          >
            Create Task
          </Button>
          <Button
            variant="ghost"
            className="px-md py-sm rounded-lg border border-outline-variant font-label-md cursor-pointer"
            onClick={onCancel}
          >
            Cancel
          </Button>
        </div>
      </div>
    </div>
  )
}
