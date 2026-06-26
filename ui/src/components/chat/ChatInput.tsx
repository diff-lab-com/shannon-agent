import { useState, useRef } from 'react'
import { useIntl } from 'react-intl'
import { open } from '@tauri-apps/plugin-dialog'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'])

function isImageFile(path: string): boolean {
  const dot = path.lastIndexOf('.')
  if (dot < 0) return false
  return IMAGE_EXTENSIONS.has(path.slice(dot + 1).toLowerCase())
}

interface ChatInputProps {
  value: string
  onChange: (value: string) => void
  onSend: () => void
  attachedFiles: string[]
  onAttach: (files: string[]) => void
  onDetachAll: () => void
  disabled: boolean
  isQuerying: boolean
  onCancelQuery: () => void
  currentSessionId: string | null
  sessionWorkingDir: string
  onOpenQuickFix: () => void
  onOpenEditor: () => void
}

export default function ChatInput({
  value,
  onChange,
  onSend,
  attachedFiles,
  onAttach,
  onDetachAll,
  disabled,
  isQuerying,
  onCancelQuery,
  currentSessionId,
  sessionWorkingDir,
  onOpenQuickFix,
  onOpenEditor,
}: ChatInputProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { config, models, refreshConfig } = useApp()
  const modelList = models ?? []
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const [isDragging, setIsDragging] = useState(false)

  const handleChangeWorkingDir = async () => {
    if (!currentSessionId) return
    try {
      const selected = await open({ directory: true, multiple: false })
      if (!selected || Array.isArray(selected)) return
      await api.setSessionWorkingDir(currentSessionId, selected as string)
      await refreshConfig()
    } catch (err) {
      console.warn('Failed to change working dir:', err)
    }
  }

  const handleModeChange = async (mode: string | null) => {
    if (!mode) return
    try {
      await api.configure({ key: 'approval_mode', value: mode })
      await refreshConfig()
    } catch (err) {
      console.warn('Failed to update approval mode:', err)
    }
  }

  const handleModelChange = async (modelId: string | null) => {
    if (!modelId) return
    const model = modelList.find(m => m.id === modelId)
    if (!model) return
    try {
      await api.configure({ key: 'model', value: model.name })
      await api.configure({ key: 'provider', value: model.provider })
      await refreshConfig()
    } catch (err) {
      console.warn('Failed to update model:', err)
    }
  }

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(true)
  }

  const handleDragLeave = (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
  }

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)

    const files: FileList = e.dataTransfer.files
    if (!files || files.length === 0) return

    const paths: string[] = []
    for (let i = 0; i < files.length; i++) {
      const file = files[i]
      if ('path' in file && typeof file.path === 'string') {
        paths.push(file.path)
      }
    }

    if (paths.length > 0) {
      onAttach(paths)
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      onSend()
    }
    if (e.key === 'Escape' && isQuerying) {
      e.preventDefault()
      onCancelQuery()
    }
  }

  const handleAttachClick = async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          { name: t('chat.input.attach.filter.images'), extensions: Array.from(IMAGE_EXTENSIONS) },
          { name: t('chat.input.attach.filter.all'), extensions: ['*'] },
        ],
      })
      if (!selected) return
      const paths = (Array.isArray(selected) ? selected : [selected]) as string[]
      if (paths.length > 0) onAttach(paths)
    } catch (err) {
      console.warn('Attach failed:', err)
    }
  }

  const currentMode = config?.approval_mode || 'suggest'
  const currentModelId = modelList.find(m => m.name === config?.model && m.provider === config?.provider)?.id || ''
  const workingDirBasename = sessionWorkingDir ? sessionWorkingDir.split('/').pop() || sessionWorkingDir.split('\\').pop() || '' : ''

  const modeOptions = [
    { value: 'readonly', label: t('chat.input.mode.readonly'), icon: 'lock', color: 'border-green-500/50' },
    { value: 'plan', label: t('chat.input.mode.plan'), icon: 'description', color: 'border-green-500/50' },
    { value: 'suggest', label: t('chat.input.mode.suggest'), icon: 'shield', color: 'border-amber-500/50' },
    { value: 'auto', label: t('chat.input.mode.auto'), icon: 'flash_auto', color: 'border-amber-500/50' },
    { value: 'full_auto', label: t('chat.input.mode.full_auto'), icon: 'bolt', color: 'border-red-500/50' },
  ]

  const selectedMode = modeOptions.find(m => m.value === currentMode) || modeOptions[2]

  return (
    <div
      className={`relative group transition-all ${isDragging ? 'ring-2 ring-primary/50 rounded-2xl' : ''}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {isDragging && (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-primary/10 rounded-2xl backdrop-blur-sm pointer-events-none">
          <div className="flex flex-col items-center gap-sm text-primary">
            <span className="material-symbols-outlined icon-xl">cloud_upload</span>
            <p className="font-label-md">{t('chat.input.attach.dropHint')}</p>
          </div>
        </div>
      )}

      <div className="flex flex-col">
        {attachedFiles.length > 0 && (
          <div className="flex flex-wrap items-center gap-xs px-md pt-md">
            {attachedFiles.map((path, i) => {
              const name = path.split(/[/\\]/).pop() || path
              const isImage = isImageFile(path)
              return (
                <span
                  key={i}
                  className="inline-flex items-center gap-xs px-sm py-xs bg-primary/10 text-primary rounded-lg font-label-sm"
                >
                  {isImage ? (
                    <img
                      src={convertFileSrc(path)}
                      alt={name}
                      className="w-5 h-5 rounded object-cover shrink-0"
                      loading="lazy"
                    />
                  ) : (
                    <span className="material-symbols-outlined text-[14px]">description</span>
                  )}
                  {name}
                  <button
                    type="button"
                    className="hover:text-error cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 rounded"
                    aria-label={t('chat.input.attach.remove')}
                    onClick={() => {
                      const newFiles = attachedFiles.filter((_, idx) => idx !== i)
                      onAttach(newFiles)
                    }}
                  >
                    <span className="material-symbols-outlined text-[14px]">close</span>
                  </button>
                </span>
              )
            })}
            {attachedFiles.length > 1 && (
              <button
                type="button"
                className="text-xs text-on-surface-variant hover:text-error cursor-pointer underline ml-xs"
                onClick={onDetachAll}
              >
                {t('chat.input.attach.detachAll')}
              </button>
            )}
          </div>
        )}

        <div className="flex items-start px-sm">
          <span className="material-symbols-outlined p-md text-primary shrink-0">
            {isQuerying ? 'hourglass_empty' : 'auto_awesome'}
          </span>
          <textarea
            ref={textareaRef}
            className="flex-1 bg-transparent border-none outline-none focus:ring-0 font-body-lg py-md px-sm placeholder:text-outline-variant/80 text-on-surface resize-none min-h-[24px] max-h-[200px]"
            placeholder={isQuerying ? t('chat.input.processing') : t('chat.input.placeholder')}
            aria-label={t('chat.input.ariaLabel')}
            value={value}
            onChange={e => onChange(e.target.value)}
            onKeyDown={handleKeyDown}
            rows={1}
            disabled={disabled}
          />
        </div>

        <div className="flex items-center justify-between gap-xs px-sm py-xs border-t border-outline-variant/20">
          <div className="flex items-center gap-xs flex-wrap min-w-0">
            <button
              type="button"
              onClick={handleChangeWorkingDir}
              disabled={!currentSessionId}
              aria-label={t('chat.input.wd.aria')}
              title={sessionWorkingDir || t('chat.input.wd.title')}
              className={`group/wd flex items-center gap-xs px-sm py-xs rounded-full text-label-sm border transition-all shrink-0 ${
                sessionWorkingDir
                  ? 'border-primary/30 bg-primary/5 text-on-surface hover:bg-primary/10 hover:border-primary/50'
                  : 'border-outline-variant/30 bg-surface-container-lowest/60 text-on-surface-variant hover:bg-surface-container-low hover:border-outline-variant hover:text-primary'
              } disabled:opacity-50 disabled:cursor-not-allowed`}
            >
              <span className="material-symbols-outlined icon-sm">folder_open</span>
              <span className="max-w-[120px] truncate font-mono">
                {workingDirBasename || t('chat.input.wd.title')}
              </span>
              <span className="material-symbols-outlined text-[14px] opacity-50 group-hover/wd:opacity-100 group-hover/wd:text-primary transition-opacity">change_folder</span>
            </button>

            <Select value={currentMode} onValueChange={handleModeChange}>
              <SelectTrigger
                size="sm"
                aria-label={t('chat.input.mode.label')}
                className={`border ${selectedMode.color} bg-transparent hover:bg-surface-container-low/50 transition-colors`}
              >
                <span className="material-symbols-outlined icon-sm">{selectedMode.icon}</span>
                <SelectValue placeholder={t('chat.input.mode.label')} />
              </SelectTrigger>
              <SelectContent>
                {modeOptions.map(mode => (
                  <SelectItem key={mode.value} value={mode.value}>
                    <div className="flex items-center gap-xs">
                      <span className="material-symbols-outlined icon-sm">{mode.icon}</span>
                      <span>{mode.label}</span>
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            <Select value={currentModelId} onValueChange={handleModelChange}>
              <SelectTrigger
                size="sm"
                aria-label={t('chat.input.model.label')}
                className="border border-outline-variant/30 bg-transparent hover:bg-surface-container-low/50 transition-colors"
              >
                <span className="material-symbols-outlined icon-sm">auto_awesome</span>
                <SelectValue placeholder={t('chat.input.model.label')} />
              </SelectTrigger>
              <SelectContent>
                {modelList.map(model => (
                  <SelectItem key={model.id} value={model.id}>
                    <div className="flex items-center gap-xs">
                      <span className="material-symbols-outlined icon-sm">auto_awesome</span>
                      <div className="flex flex-col">
                        <span className="text-sm">{model.name}</span>
                        <span className="text-xs text-on-surface-variant">{model.provider}</span>
                      </div>
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex items-center gap-xs shrink-0">
            <Button
              variant="ghost"
              aria-label={t('chat.input.attach.aria')}
              title={t('chat.input.attach.aria')}
              className="p-md text-on-surface-variant hover:text-primary"
              onClick={handleAttachClick}
            >
              <span className="material-symbols-outlined icon-md">attach_file</span>
            </Button>

            <Button
              variant="ghost"
              aria-label={t('nav.quickFix')}
              title={t('nav.quickFix')}
              className="p-md text-on-surface-variant hover:text-primary"
              onClick={onOpenQuickFix}
            >
              <span className="material-symbols-outlined icon-md">build</span>
            </Button>

            <Button
              variant="ghost"
              aria-label={t('nav.editor')}
              title={t('nav.editor')}
              className="p-md text-on-surface-variant hover:text-primary"
              onClick={onOpenEditor}
            >
              <span className="material-symbols-outlined icon-md">code</span>
            </Button>

            {isQuerying ? (
              <Button
                aria-label={t('chat.input.stop.aria')}
                className="bg-error/80 text-on-error p-3 rounded-xl active:scale-95 transition-all"
                onClick={onCancelQuery}
              >
                <span className="material-symbols-outlined icon-md">stop</span>
              </Button>
            ) : (
              <Button
                aria-label={t('chat.input.send.aria')}
                className="bg-primary text-on-primary p-3 rounded-xl active:scale-95 hover:shadow-md hover:shadow-primary/30 transition-all disabled:opacity-40 disabled:cursor-not-allowed"
                onClick={onSend}
                disabled={!value.trim()}
              >
                <span className="material-symbols-outlined icon-md">arrow_upward</span>
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
