import { Suspense, type ComponentType } from 'react'
import LoadingState from '@/components/ui/loading-state'
import { Button } from '@/components/ui/button'
import { Modal } from '@/components/ui/modal'
import { useT } from '@/i18n'

interface InlinePanelModalProps<P extends object> {
  open: boolean
  onClose: () => void
  /** Title shown in the sticky modal header. */
  title: string
  /** Lazy-loaded panel component (mounted via React.lazy). */
  panel: ComponentType<P>
  /** Props forwarded to the panel (e.g. Editor's `initialPath`). */
  panelProps?: P
  /** Outer modal size preset. */
  size: '2xl' | 'xl' | 'lg' | 'md' | 'sm'
  /** Modal root classes — controls max-width / height / overflow of the dialog box. */
  modalClassName: string
  /** Body container classes — `'p-lg'` for scrollable content, `'flex-1 overflow-hidden'` for embedded editors. */
  bodyClassName: string
}

const LoadingFallback = () => <LoadingState />

export default function InlinePanelModal<P extends object>({
  open,
  onClose,
  title,
  panel: Panel,
  panelProps,
  size,
  modalClassName,
  bodyClassName,
}: InlinePanelModalProps<P>) {
  const t = useT()
  return (
    <Modal
      open={open}
      onClose={onClose}
      size={size}
      showCloseButton={false}
      className={modalClassName}
    >
      <div className="flex items-center justify-between px-lg py-md bg-surface-container-lowest/95 backdrop-blur-md border-b border-outline-variant/20">
        <h3 className="font-headline-md text-on-surface">{title}</h3>
        <Button variant="ghost" aria-label={t('chat.delete.cancel')} onClick={onClose}>
          <span className="material-symbols-outlined">close</span>
        </Button>
      </div>
      <div className={bodyClassName}>
        <Suspense fallback={<LoadingFallback />}>
          {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
          <Panel {...((panelProps ?? {}) as any)} />
        </Suspense>
      </div>
    </Modal>
  )
}