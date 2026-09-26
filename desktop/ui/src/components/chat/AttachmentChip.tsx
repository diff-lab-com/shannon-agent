import { useIntl } from 'react-intl'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'

interface AttachmentChipProps {
  path: string
  onRemove: () => void
}

// B4 P2-11: the remove affordance is announced with the file's name; the
// dead `size`/`formatSize` display (no consumer ever passed one) is gone.
export function AttachmentChip({ path, onRemove }: AttachmentChipProps) {
  const intl = useIntl()
  const name = path.split(/[/\\]/).pop() || path
  const image = /\.(png|jpe?g|webp|gif)$/i.test(name)
  return (
    <span className="inline-flex max-w-[240px] items-center gap-xs rounded-lg bg-primary/10 px-sm py-xs text-primary font-label-sm">
      {image ? <img src={convertFileSrc(path)} alt={name} className="h-5 w-5 shrink-0 rounded object-cover" /> : <span className="material-symbols-outlined text-[14px]">description</span>}
      <span className="truncate">{name}</span>
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        aria-label={intl.formatMessage({ id: 'chat.input.attach.remove' }, { name })}
        title={intl.formatMessage({ id: 'chat.input.attach.remove' }, { name })}
        onClick={onRemove}
        className="hover:text-error"
      >
        <span className="material-symbols-outlined text-[14px]">close</span>
      </Button>
    </span>
  )
}

export default AttachmentChip
