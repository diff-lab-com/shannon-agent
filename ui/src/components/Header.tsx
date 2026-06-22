import { useState, useEffect, useRef } from 'react';
import { useIntl } from 'react-intl';
import { useLocation } from 'react-router-dom';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useApp } from '@/context/AppContext';
import { useSidebar } from './Layout';
import * as api from '@/lib/tauri-api';

const TITLE_MAP: [string, string][] = [
  ['/opc/task', 'OPC Task'],
  ['/opc', 'One Person Company'],
  ['/settings', 'Settings'],
  ['/tasks', 'Scheduled'],
  ['/extensions', 'Extensions'],
  ['/chat', 'Chat'],
]

function getTitle(pathname: string): string {
  for (const [prefix, title] of TITLE_MAP) {
    if (pathname.includes(prefix)) return title
  }
  return 'Chat'
}

export function Header() {
  const intl = useIntl()
  const t = (id: string, values?: any) => intl.formatMessage({ id }, values)
  const location = useLocation();
  const { status, models, permissionRequest, respondPermission } = useApp();
  const { toggle: toggleSidebar } = useSidebar();
  const [modelOpen, setModelOpen] = useState(false);
  const modelRef = useRef<HTMLDivElement>(null);
  const [modelFocus, setModelFocus] = useState(-1);

  const title = getTitle(location.pathname);
  const isOpc = location.pathname.includes('/opc') && !location.pathname.includes('/opc/task');
  const isOpcTask = location.pathname.includes('/opc/task');

  // Click outside to close model selector
  useEffect(() => {
    if (!modelOpen) return
    const handleClick = (e: MouseEvent) => {
      if (modelRef.current && !modelRef.current.contains(e.target as Node)) {
        setModelOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [modelOpen])

  const handleModelSwitch = async (modelId: string) => {
    if (!status) return
    try {
      await api.switchProvider({ provider: status.provider, model: modelId })
      setModelOpen(false)
      toast.success(t('header.permRequest.toast.switched', { modelId }))
    } catch (e) { console.warn("Header error:", e); toast.error(t('header.permRequest.error')) }
  }

  return (
    <>
      <header className="fixed top-0 right-0 z-40 flex justify-between items-center h-16 px-lg bg-surface/80 backdrop-blur-md shadow-sm border-b border-outline-variant/10" style={{ left: 'var(--sidebar-w)' }}>
        <Button variant="ghost" aria-label="Toggle sidebar" className="md:hidden p-2 mr-sm text-on-surface-variant hover:text-primary" onClick={toggleSidebar}>
          <span className="material-symbols-outlined text-[24px]">menu</span>
        </Button>
        <div className="flex items-center gap-md relative w-full overflow-hidden">
          {isOpcTask ? (
            <div className="flex items-center gap-2">
              <span className="material-symbols-outlined text-primary text-[28px]">auto_awesome</span>
              <h2 className="font-headline-md text-[24px] font-extrabold text-primary whitespace-nowrap">{title}</h2>
            </div>
          ) : (
            <h2 className="font-headline-md text-[24px] font-extrabold text-on-surface whitespace-nowrap">{title}</h2>
          )}

          {isOpc && (
            <div className="flex-1 max-w-[400px] ml-auto mr-lg relative hidden md:block">
              <Input
                type="text"
                placeholder={t('header.search.placeholder')}
                className="w-full bg-surface-container-low border-none rounded-full py-2 pl-4 pr-10 text-sm font-body-md focus:ring-2 focus:ring-primary/20 transition-all outline-none"
              />
              <span className="material-symbols-outlined absolute right-3 top-1/2 -translate-y-1/2 text-on-surface-variant text-[20px]">search</span>
            </div>
          )}

          {isOpcTask && (
            <div className="ml-auto mr-lg flex items-center gap-2 bg-surface-container-low px-3 py-1.5 rounded-full border border-outline-variant/20 shrink-0">
               <span className="w-2 h-2 rounded-full bg-tertiary"></span>
               <span className="font-label-sm text-[12px] text-on-surface-variant whitespace-nowrap">{t('header.syncStatus')}</span>
            </div>
          )}
        </div>
        <div className="flex items-center gap-lg shrink-0 pl-4 border-l border-outline-variant/20 md:border-none md:pl-0">
          {/* Model selector */}
          <div className="relative" ref={modelRef}>
            <Button
              variant="ghost"
              aria-label={t('header.model.select')}
              className="flex items-center gap-sm px-md py-sm rounded-lg hover:bg-surface-container-low text-on-surface-variant hover:text-primary transition-all"
              onClick={() => { setModelOpen(!modelOpen); setModelFocus(-1) }}
            >
              <span className={`w-2 h-2 rounded-full shrink-0 ${status?.querying ? 'bg-secondary animate-pulse' : 'bg-tertiary'}`}></span>
              <span className="font-label-sm text-[12px] whitespace-nowrap max-w-[120px] truncate">{status?.model || t('header.model.noModel')}</span>
              <span className="material-symbols-outlined text-[16px]">expand_more</span>
            </Button>
            {modelOpen && models.length > 0 && (
              <div className="absolute right-0 top-full mt-sm w-[280px] bg-surface-container-lowest/95 backdrop-blur-lg rounded-xl border border-outline-variant/20 shadow-xl z-50 py-sm" role="listbox" onKeyDown={e => {
                if (e.key === 'ArrowDown') { e.preventDefault(); setModelFocus(f => Math.min(f + 1, models.length - 1)) }
                else if (e.key === 'ArrowUp') { e.preventDefault(); setModelFocus(f => Math.max(f - 1, 0)) }
                else if (e.key === 'Enter' && modelFocus >= 0) { handleModelSwitch(models[modelFocus].id) }
                else if (e.key === 'Escape') { setModelOpen(false) }
              }}>
                {models.map((m, i) => (
                  <button
                    key={m.id}
                    role="option"
                    aria-selected={m.id === status?.model}
                    className={`w-full text-left px-md py-sm flex items-center justify-between transition-colors ${i === modelFocus ? 'bg-primary/10 text-primary' : m.id === status?.model ? 'text-primary font-bold' : 'text-on-surface hover:bg-primary/5'}`}
                    onClick={() => handleModelSwitch(m.id)}
                    onMouseEnter={() => setModelFocus(i)}
                  >
                    <span className="font-label-md truncate">{m.name}</span>
                    <span className="text-label-sm text-on-surface-variant">{m.context_window > 0 ? `${(m.context_window / 1000).toFixed(0)}k` : ''}</span>
                  </button>
                ))}
              </div>
            )}
          </div>

          <Button variant="ghost" aria-label={t('header.notifications')} title={t('header.notifications.aria')} className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant hover:text-primary transition-colors relative">
            <span className="material-symbols-outlined text-[20px]" aria-hidden="true">notifications</span>
          </Button>
          <Button variant="ghost" aria-label={t('header.help')} title={t('header.help.aria')} className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant hover:text-primary transition-colors" onClick={() => window.dispatchEvent(new CustomEvent('shannon:toggle-help'))}>
            <span className="material-symbols-outlined text-[20px]" aria-hidden="true">help</span>
          </Button>
          <div className="h-8 w-8 rounded-full overflow-hidden bg-surface-container flex items-center justify-center ring-2 ring-primary/10">
            <span className="material-symbols-outlined text-on-surface-variant text-[18px]" aria-hidden="true">person</span>
          </div>
        </div>
      </header>

      {/* Permission Modal */}
      {permissionRequest && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/30 backdrop-blur-sm" onKeyDown={e => { if (e.key === 'Escape') respondPermission(permissionRequest.request_id, false) }}>
          <div className="bg-surface-container-lowest rounded-2xl shadow-2xl border border-outline-variant/20 p-xl max-w-md w-full mx-md" role="dialog" aria-modal="true">
            <div className="flex items-center gap-md mb-lg">
              <div className="h-10 w-10 rounded-full bg-tertiary-container flex items-center justify-center">
                <span className="material-symbols-outlined text-on-tertiary-container">shield</span>
              </div>
              <div className="flex-1">
                <h3 className="font-headline-sm text-on-surface font-bold">{t('header.permRequest.title')}</h3>
                <p className="text-body-sm text-on-surface-variant">{t('header.permRequest.subtitle')}</p>
              </div>
              <span className={`px-sm py-xs rounded-full font-label-sm font-bold uppercase tracking-wider ${
                permissionRequest.risk === 'critical' ? 'bg-error/10 text-error' :
                permissionRequest.risk === 'high' ? 'bg-secondary/10 text-secondary' :
                permissionRequest.risk === 'medium' ? 'bg-secondary/10 text-secondary' :
                'bg-tertiary/10 text-tertiary'
              }`}>{permissionRequest.risk}</span>
            </div>
            <div className="p-md bg-surface-container-low rounded-xl mb-lg space-y-sm">
              <div className="flex justify-between">
                <span className="text-label-sm text-on-surface-variant">{t('header.permRequest.tool')}</span>
                <span className="font-label-md text-on-surface font-bold">{permissionRequest.tool}</span>
              </div>
              {permissionRequest.input ? (
                <pre className="text-body-sm text-on-surface-variant bg-surface-container p-sm rounded-lg overflow-x-auto max-h-[200px] mt-sm">{JSON.stringify(permissionRequest.input as object, null, 2)}</pre>
              ) : null}
            </div>
            <label className="flex items-center gap-sm mb-lg cursor-pointer text-body-sm text-on-surface-variant hover:text-on-surface transition-colors">
              <input type="checkbox" className="w-4 h-4 rounded border-outline-variant text-primary focus:ring-primary/30" />
              {t('header.permRequest.alwaysAllow')}
            </label>
            <div className="flex gap-md">
              <Button autoFocus className="flex-1 py-sm bg-surface-container text-on-surface rounded-xl hover:bg-surface-container-high transition-all font-label-md" onClick={() => respondPermission(permissionRequest.request_id, false)}>
                {t('header.permRequest.deny')}
              </Button>
              <Button className="flex-1 py-sm bg-primary text-on-primary rounded-xl hover:shadow-md hover:shadow-primary/30 active:scale-95 transition-all font-label-md" onClick={() => respondPermission(permissionRequest.request_id, true)}>
                {t('header.permRequest.allowOnce')}
              </Button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
