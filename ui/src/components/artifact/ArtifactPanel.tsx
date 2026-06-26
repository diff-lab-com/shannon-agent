import { useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { save } from '@tauri-apps/plugin-dialog'
import * as api from '@/lib/tauri-api'
import { useArtifact } from './ArtifactContext'
import { artifactIcon, artifactKindLabel } from './detectArtifact'
import { HtmlRenderer } from './HtmlRenderer'
import { SvgRenderer } from './SvgRenderer'
import { MermaidRenderer } from './MermaidRenderer'
import { DocumentRenderer } from './DocumentRenderer'

type Tab = 'preview' | 'code'

const FILE_EXT: Record<string, string> = {
  html: 'html',
  svg: 'svg',
  mermaid: 'mmd',
  document: 'md',
}

export function ArtifactPanel() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { artifacts, activeId, setActive, closeAll } = useArtifact()
  const [tab, setTab] = useState<Tab>('preview')

  if (artifacts.length === 0) return null
  const active = artifacts.find(a => a.id === activeId) ?? artifacts[artifacts.length - 1]

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(active.source)
      toast.success(t('chat.artifact.copied'))
    } catch {
      toast.error(t('chat.artifact.copyFailed'))
    }
  }

  const handleExport = async () => {
    const ext = FILE_EXT[active.kind] || 'txt'
    try {
      const path = await save({
        defaultPath: `${active.title.replace(/[^a-zA-Z0-9-_]+/g, '_').slice(0, 60) || 'artifact'}.${ext}`,
        filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
      })
      if (!path) return
      await api.saveTextFile(path, active.source)
      toast.success(t('chat.artifact.exported'))
    } catch (err) {
      console.warn('Export failed:', err)
      toast.error(t('chat.artifact.exportFailed'))
    }
  }

  return (
    <aside
      role="complementary"
      aria-label={t('chat.artifact.panel.aria')}
      className="shrink-0 overflow-hidden border-l border-outline-variant/20 bg-surface-container-lowest flex flex-col"
      style={{ width: 'min(640px, 45vw)' }}
    >
      <header className="flex items-center gap-sm px-md py-sm border-b border-outline-variant/20">
        <span className="material-symbols-outlined icon-sm text-primary shrink-0">{artifactIcon(active.kind)}</span>
        <span className="font-label-md text-on-surface truncate flex-1">{active.title}</span>
        <span className="font-label-xs text-on-surface-variant px-xs py-[2px] rounded bg-surface-container-high shrink-0">
          {artifactKindLabel(active.kind)}
        </span>
        <button
          type="button"
          onClick={closeAll}
          aria-label={t('chat.artifact.close.aria')}
          title={t('chat.artifact.close.aria')}
          className="p-xs rounded text-on-surface-variant hover:text-on-surface hover:bg-surface-container focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        >
          <span className="material-symbols-outlined icon-sm">close</span>
        </button>
      </header>

      {artifacts.length > 1 && (
        <div role="tablist" aria-label={t('chat.artifact.tabs.aria')} className="flex gap-xs px-md py-xs overflow-x-auto border-b border-outline-variant/10 bg-surface-container-low/50">
          {artifacts.map(a => (
            <button
              key={a.id}
              type="button"
              role="tab"
              aria-selected={a.id === active.id}
              onClick={() => setActive(a.id)}
              className={`shrink-0 px-sm py-xs rounded-md text-label-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 ${
                a.id === active.id
                  ? 'bg-primary/10 text-primary'
                  : 'text-on-surface-variant hover:bg-surface-container hover:text-on-surface'
              }`}
            >
              <span className="material-symbols-outlined icon-sm align-middle mr-xs">{artifactIcon(a.kind)}</span>
              <span className="align-middle">{a.title.slice(0, 30)}</span>
            </button>
          ))}
        </div>
      )}

      <div className="flex items-center gap-xs px-md py-xs border-b border-outline-variant/10 bg-surface-container-lowest">
        {(['preview', 'code'] as Tab[]).map(tb => (
          <button
            key={tb}
            type="button"
            role="tab"
            aria-selected={tab === tb}
            onClick={() => setTab(tb)}
            className={`px-sm py-xs rounded-md text-label-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 ${
              tab === tb
                ? 'bg-primary/10 text-primary'
                : 'text-on-surface-variant hover:bg-surface-container hover:text-on-surface'
            }`}
          >
            {t(`chat.artifact.tab.${tb}`)}
          </button>
        ))}
        <div className="flex-1" />
        <button
          type="button"
          onClick={handleCopy}
          className="flex items-center gap-xs px-sm py-xs rounded-md text-label-sm text-on-surface-variant hover:bg-surface-container hover:text-on-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        >
          <span className="material-symbols-outlined icon-sm">content_copy</span>
          {t('chat.artifact.copy')}
        </button>
        <button
          type="button"
          onClick={handleExport}
          className="flex items-center gap-xs px-sm py-xs rounded-md text-label-sm text-on-surface-variant hover:bg-surface-container hover:text-on-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        >
          <span className="material-symbols-outlined icon-sm">download</span>
          {t('chat.artifact.export')}
        </button>
      </div>

      <div className="flex-1 overflow-auto">
        {tab === 'preview' ? (
          active.kind === 'html' ? <HtmlRenderer source={active.source} title={active.title} />
          : active.kind === 'svg' ? <SvgRenderer source={active.source} title={active.title} />
          : active.kind === 'mermaid' ? <MermaidRenderer source={active.source} title={active.title} />
          : active.kind === 'document' ? <DocumentRenderer source={active.source} />
          : (
            <div className="p-md text-body-sm text-on-surface-variant font-mono whitespace-pre-wrap break-words">
              {t('chat.artifact.previewUnsupported')}
            </div>
          )
        ) : (
          <pre className="p-md text-body-sm font-mono text-on-surface whitespace-pre-wrap break-words">{active.source}</pre>
        )}
      </div>
    </aside>
  )
}
