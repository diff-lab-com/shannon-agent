import { convertFileSrc } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'

interface AttachmentChipProps {
  path: string
  size?: number
  onRemove: () => void
}

function formatSize(size?: number): string {
  if (size == null) return ''
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}

export function AttachmentChip({ path, size, onRemove }: AttachmentChipProps) {
  const name = path.split(/[/\\]/).pop() || path
  const image = /\.(png|jpe?g|webp|gif)$/i.test(name)
  return (
    <span className="inline-flex max-w-[240px] items-center gap-xs rounded-lg bg-primary/10 px-sm py-xs text-primary font-label-sm">
      {image ? <img src={convertFileSrc(path)} alt={name} className="h-5 w-5 shrink-0 rounded object-cover" /> : <span className="material-symbols-outlined text-[14px]">description</span>}
      <span className="truncate">{name}</span>
      {size != null && <span className="shrink-0 text-on-surface-variant">{formatSize(size)}</span>}
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        aria-label={`Remove ${name}`}
        onClick={onRemove}
        className="hover:text-error"
      >
        <span className="material-symbols-outlined text-[14px]">close</span>
      </Button>
    </span>
  )
}

export default AttachmentChip
