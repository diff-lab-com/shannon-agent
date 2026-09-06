/**
 * P1-5 D — integrated terminal panel (chat page bottom drawer).
 *
 * xterm.js renders the PTY byte stream the Rust pump streams over the
 * `terminal:output` event (base64 → decoded here, ≤16 ms coalesced). All
 * process operations go through the frozen `terminal_*` commands; this
 * component never touches a shell surface itself.
 *
 * UX contract (brief):
 *  - bottom drawer in the chat page, fixed ~320 px, full-height toggle;
 *    NO free-form drag layout (that is T11);
 *  - ≤4 terminal tabs (backend enforces the same cap);
 *  - Ctrl+` toggles the panel — registered here (not in the global
 *    useKeyboardShortcuts map) because xterm's hidden textarea would be
 *    skipped by that hook's INPUT/TEXTAREA guard. Checked against the
 *    registered global shortcuts (Ctrl+Shift+S/N/K) and the in-app map
 *    (mod+n/k/d/1-6, ?, /): Ctrl+` was free;
 *  - theme follows the app theme registry (live tokens, dark/light ANSI
 *    floor — see ./xtermTheme);
 *  - reconnect: `terminal_list` restores live tabs, but output history
 *    from before the reconnect is gone (v1 has no replay buffer) — the
 *    panel says so explicitly;
 *  - a11y: labelled region, tablist semantics, focus moves into the
 *    terminal on open and back to the toggle button on close.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useIntl, type PrimitiveType } from 'react-intl';
import '@xterm/xterm/css/xterm.css';
import * as api from '@/lib/tauri-api';
import type { TerminalInfo } from '@/types';
import { bytesContainAscii, decodeTerminalOutput, listenTerminalOutput } from '@/lib/runtime/terminalEvents';
import { xtermTheme } from './xtermTheme';
import type { Terminal as XTerm } from '@xterm/xterm';
import type { FitAddon as XTermFitAddon } from '@xterm/addon-fit';

/**
 * Resolved theme id read straight off `<html data-theme>` (what
 * ThemeProvider mirrors). Deliberately NOT `useTheme()`: the panel must be
 * mountable anywhere — including test trees that render the Chat page
 * without a ThemeProvider — and the attribute is observable.
 */
function useResolvedThemeAttr(): string {
  const read = () =>
    typeof document === 'undefined' ? 'material' : document.documentElement.getAttribute('data-theme') ?? 'material';
  const [theme, setTheme] = useState(read);
  useEffect(() => {
    if (typeof MutationObserver === 'undefined') return;
    const observer = new MutationObserver(() => setTheme(read()));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => observer.disconnect();
  }, []);
  return theme;
}

/** Backend cap (terminal_commands.rs MAX_TERMINALS) mirrored for the UI. */
const MAX_TERMINALS = 4;

/** Default drawer height (brief: ~320px). */
const DRAWER_HEIGHT_PX = 320;

/** Monospace fallback stack (brief: 字体回退等宽栈). */
const FONT_FAMILY =
  "'JetBrains Mono', 'Fira Code', 'Cascadia Code', Menlo, Consolas, 'DejaVu Sans Mono', monospace";

interface TerminalTab {
  info: TerminalInfo;
  /** Backend announced the process exited (tab lingers for review). */
  exited: boolean;
}

interface TermEntry {
  term: XTerm;
  fit: XTermFitAddon;
  detachOutput: () => void;
}

/** basename(3)-ish label so tabs read "shannon-lane-f" not "/home/…". */
function dirLabel(projectDir: string): string {
  const trimmed = projectDir.replace(/\/+$/, '');
  const base = trimmed.split('/').pop() ?? trimmed;
  return base.length > 0 ? base : projectDir;
}

export interface TerminalPanelProps {
  projectDir?: string | null
  /**
   * P1-5 C-2 — `drawer` (default) is the chat-page bottom drawer.
   * `panel` renders the same component as an always-open WorkspaceGrid
   * panel: it fills its container, skips the drawer toggle affordances and
   * the Ctrl+` window handler (the grid host owns panel presence instead).
   * The Chat page portals ONE instance between the two containers, so xterm
   * instances (and their scrollback) survive the drawer↔grid handoff, and
   * the Rust-side TerminalManager keeps processes alive throughout.
   */
  variant?: 'drawer' | 'panel'
}

export function TerminalPanel({ projectDir, variant = 'drawer' }: TerminalPanelProps) {
  const embedded = variant === 'panel'
  const intl = useIntl();
  const t = useCallback(
    (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values),
    [intl],
  );
  const resolvedTheme = useResolvedThemeAttr();

  const [openState, setOpen] = useState(false);
  // Grid-embedded panels are always open; the drawer keeps its own state.
  const open = embedded || openState;
  const [fullHeight, setFullHeight] = useState(false);
  const [tabs, setTabs] = useState<TerminalTab[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [showHistoryHint, setShowHistoryHint] = useState(true);
  const [booted, setBooted] = useState(false);

  const containerRef = useRef<HTMLDivElement>(null);
  const toggleRef = useRef<HTMLButtonElement>(null);
  const termsRef = useRef<Map<string, TermEntry>>(new Map());
  // xterm is loaded on first mount, not at module import: the chat bundle
  // stays free of xterm until a terminal is actually opened, and jsdom
  // test trees never load it at all (its module init touches canvas).
  const xtermModuleRef = useRef<{
    Terminal: new (options: ConstructorParameters<typeof Object>[0] & Record<string, unknown>) => XTerm;
    FitAddon: new () => XTermFitAddon;
  } | null>(null);
  const [xtermReady, setXtermReady] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void Promise.all([import('@xterm/xterm'), import('@xterm/addon-fit')]).then(
      ([xterm, fit]) => {
        if (cancelled) return;
        xtermModuleRef.current = { Terminal: xterm.Terminal, FitAddon: fit.FitAddon };
        setXtermReady(true);
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  const activeTab = useMemo(
    () => tabs.find(tab => tab.info.terminalId === activeId) ?? null,
    [tabs, activeId],
  );

  // ── xterm instance lifecycle ────────────────────────────────────────

  const ensureTerm = useCallback((info: TerminalInfo): TermEntry | null => {
    const existing = termsRef.current.get(info.terminalId);
    if (existing) return existing;
    const xterm = xtermModuleRef.current;
    if (!xterm) return null; // module still loading — next effect run mounts

    const fit = new xterm.FitAddon();
    const term = new xterm.Terminal({
      convertEol: false,
      cursorBlink: true,
      fontFamily: FONT_FAMILY,
      fontSize: 12,
      scrollback: 5000,
      theme: xtermTheme(resolvedTheme),
    });
    term.loadAddon(fit);

    term.onData(data => {
      void api.terminalWrite(info.terminalId, data).catch(() => {
        /* dead terminal: the exit notice is already on screen */
      });
    });
    term.onResize(({ cols, rows }) => {
      void api.terminalResize(info.terminalId, cols, rows).catch(() => {});
    });

    // listenTerminalOutput resolves asynchronously (Tauri listen); a
    // disposed entry unsubscribes immediately on resolution so an
    // unmount-before-subscribe race never leaks a listener.
    let detach: (() => void) | null = null;
    let disposed = false;
    void listenTerminalOutput(payload => {
      if (payload.terminalId !== info.terminalId) return;
      // Raw bytes go straight to xterm: the pump slices the pty stream at
      // arbitrary byte boundaries, and xterm's write buffer completes
      // multi-byte sequences split across events. Decoding per event would
      // turn both halves of a split sequence into U+FFFD.
      const bytes = decodeTerminalOutput(payload.data);
      term.write(bytes);
      if (bytesContainAscii(bytes, '[shannon: process exited')) {
        setTabs(prev => prev.map(tab => (
          tab.info.terminalId === info.terminalId ? { ...tab, exited: true } : tab
        )));
      }
    }).then(fn => {
      if (disposed) {
        fn();
        return;
      }
      detach = fn;
    });
    const detachOutput = () => {
      disposed = true;
      detach?.();
    };

    const entry: TermEntry = { term, fit, detachOutput };
    termsRef.current.set(info.terminalId, entry);
    return entry;
    // resolvedTheme is intentionally not a dependency: instances keep
    // their creation theme and are re-themed wholesale by the effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Re-theme every live terminal when the app theme changes (the
  // data-theme attribute mutation drives this via useResolvedThemeAttr).
  useEffect(() => {
    const theme = xtermTheme(resolvedTheme);
    termsRef.current.forEach(({ term }) => {
      term.options.theme = theme;
    });
  }, [resolvedTheme]);

  // Mount/unmount the active terminal into the drawer body.
  useEffect(() => {
    const container = containerRef.current;
    if (!open || !container || !activeTab || !xtermReady) return;
    const entry = ensureTerm(activeTab.info);
    if (!entry) return;
    const { term, fit } = entry;
    if (!term.element) {
      term.open(container);
    } else if (term.element.parentElement !== container) {
      container.appendChild(term.element);
    }
    try {
      fit.fit();
    } catch {
      // jsdom/mocked fit has no dimensions — harmless.
    }
    term.scrollToBottom();
    term.focus();
  }, [open, activeTab, ensureTerm, xtermReady]);

  // ResizeObserver → fit → onResize → terminal_resize IPC.
  useEffect(() => {
    const container = containerRef.current;
    if (!open || !container || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => {
      if (!activeId) return;
      const entry = termsRef.current.get(activeId);
      if (!entry) return;
      try {
        entry.fit.fit();
      } catch {
        /* no measurable layout yet */
      }
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, [open, activeId]);

  // Detach everything on unmount (panel leaves the tree with the page).
  useEffect(() => () => {
    termsRef.current.forEach(({ detachOutput, term, fit }) => {
      detachOutput();
      fit.dispose();
      term.dispose();
    });
    termsRef.current.clear();
  }, []);

  // ── Session lifecycle ───────────────────────────────────────────────

  const spawnTab = useCallback(async (dir?: string | null) => {
    const { terminalId } = await api.terminalSpawn(dir ?? projectDir ?? null);
    const info: TerminalInfo = {
      terminalId,
      projectDir: dir ?? projectDir ?? '',
      shell: '',
      startedAtMs: Date.now(),
    };
    // The backend list is authoritative for shell/startedAt; merge the
    // spawn response in first so the tab appears instantly.
    setTabs(prev => {
      if (prev.some(tab => tab.info.terminalId === terminalId)) return prev;
      return [...prev, { info, exited: false }];
    });
    setActiveId(terminalId);
    void api.terminalList().then(list => {
      const fresh = list.find(entry => entry.terminalId === terminalId);
      if (!fresh) return;
      setTabs(prev => prev.map(tab => (
        tab.info.terminalId === terminalId ? { ...tab, info: fresh } : tab
      )));
    }).catch(() => {});
    return terminalId;
  }, [projectDir]);

  /** Open the drawer; first open reconciles with the backend. */
  const openPanel = useCallback(async () => {
    setOpen(true);
    if (booted) return;
    setBooted(true);
    try {
      const list = await api.terminalList();
      const known = list.slice(0, MAX_TERMINALS).map(info => ({ info, exited: false }));
      setTabs(known);
      if (known.length > 0) {
        setActiveId(known[known.length - 1].info.terminalId);
        setShowHistoryHint(true);
        return;
      }
      await spawnTab();
    } catch {
      // Backend unreachable: the drawer still opens and shows the error
      // state via the empty tab list (retry through the + button).
    }
  }, [booted, spawnTab]);

  const togglePanel = useCallback(() => {
    if (open) {
      setOpen(false);
      // Focus management: return focus to the toggle button.
      requestAnimationFrame(() => toggleRef.current?.focus());
    } else {
      void openPanel();
    }
  }, [open, openPanel]);

  // Ctrl+` — capture phase so it wins over xterm's own key handling.
  // Skipped when embedded in a workspace grid panel: the grid host owns
  // whether the terminal panel exists at all (its close button removes the
  // panel), so the shortcut must not fight the layout.
  useEffect(() => {
    if (embedded) return;
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key === '`') {
        e.preventDefault();
        e.stopPropagation();
        togglePanel();
      }
    };
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [embedded, togglePanel]);

  const closeTab = useCallback((terminalId: string) => {
    void api.terminalKill(terminalId).catch(() => {});
    const entry = termsRef.current.get(terminalId);
    if (entry) {
      entry.detachOutput();
      entry.fit.dispose();
      entry.term.dispose();
      termsRef.current.delete(terminalId);
    }
    setTabs(prev => {
      const next = prev.filter(tab => tab.info.terminalId !== terminalId);
      setActiveId(current => {
        if (current !== terminalId) return current;
        return next.length > 0 ? next[next.length - 1].info.terminalId : null;
      });
      return next;
    });
  }, []);

  const canSpawn = tabs.length < MAX_TERMINALS;

  if (!open) {
    return (
      <div className="shrink-0 flex justify-end px-md pb-xs">
        <button
          ref={toggleRef}
          type="button"
          onClick={togglePanel}
          aria-expanded={false}
          aria-label={t('terminal.panel.toggle')}
          title={t('terminal.panel.toggle')}
          className="inline-flex items-center gap-xs rounded-full border border-outline-variant/40 bg-surface-container-low px-sm py-1 font-label-sm text-label-sm text-on-surface-variant hover:bg-surface-container focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">terminal</span>
          {t('terminal.title')}
        </button>
      </div>
    );
  }

  return (
    <section
      role="region"
      aria-label={t('terminal.panel.label')}
      data-terminal-variant={variant}
      className={`flex flex-col bg-surface-container-lowest ${
        embedded
          ? 'h-full min-h-0'
          : `shrink-0 border-t border-outline-variant/30 ${fullHeight ? 'flex-1 min-h-0' : ''}`
      }`}
      style={!embedded && !fullHeight ? { height: DRAWER_HEIGHT_PX } : undefined}
    >
      {/* Toolbar: tabs + actions */}
      <div className="flex items-center gap-xs px-sm py-1 border-b border-outline-variant/20 bg-surface-container-low/60">
        <div role="tablist" aria-label={t('terminal.panel.label')} className="flex items-center gap-xs flex-1 min-w-0 overflow-x-auto">
          {tabs.map(tab => {
            const selected = tab.info.terminalId === activeId;
            return (
              <div key={tab.info.terminalId} className="flex items-center shrink-0">
                <button
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  onClick={() => setActiveId(tab.info.terminalId)}
                  className={`px-sm py-1 rounded-t-md font-label-sm text-label-sm flex items-center gap-xs ${
                    selected
                      ? 'bg-surface-container-lowest text-on-surface border border-b-0 border-outline-variant/30'
                      : 'text-on-surface-variant hover:bg-surface-container'
                  }`}
                >
                  <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                    {tab.exited ? 'sleep' : 'terminal'}
                  </span>
                  {dirLabel(tab.info.projectDir || tab.info.shell || tab.info.terminalId.slice(0, 8))}
                  {tab.exited && <span className="text-outline">· {t('terminal.exited')}</span>}
                </button>
                <button
                  type="button"
                  onClick={() => closeTab(tab.info.terminalId)}
                  aria-label={`${t('terminal.tab.close')}: ${dirLabel(tab.info.projectDir)}`}
                  className="ml-0.5 p-0.5 rounded text-on-surface-variant hover:text-error hover:bg-surface-container focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                >
                  <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
                </button>
              </div>
            );
          })}
        </div>
        <button
          type="button"
          onClick={() => void spawnTab()}
          disabled={!canSpawn}
          aria-label={t('terminal.tab.new')}
          title={canSpawn ? t('terminal.tab.new') : t('terminal.maxReached', { max: MAX_TERMINALS })}
          className="p-1 rounded text-on-surface-variant hover:bg-surface-container disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">add</span>
        </button>
        {!embedded && (
          <button
            type="button"
            onClick={() => setFullHeight(p => !p)}
            aria-pressed={fullHeight}
            aria-label={t('terminal.panel.fullHeight')}
            title={t('terminal.panel.fullHeight')}
            className="p-1 rounded text-on-surface-variant hover:bg-surface-container focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">
              {fullHeight ? 'collapse_content' : 'expand_content'}
            </span>
          </button>
        )}
        <button
          ref={toggleRef}
          type="button"
          onClick={togglePanel}
          aria-expanded
          aria-label={t('terminal.panel.close')}
          title={t('terminal.panel.toggle')}
          className="p-1 rounded text-on-surface-variant hover:bg-surface-container focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">keyboard_hide</span>
        </button>
      </div>

      {showHistoryHint && (
        <div className="flex items-center gap-xs px-sm py-1 bg-surface-container/70 border-b border-outline-variant/20">
          <span className="material-symbols-outlined icon-sm text-on-surface-variant" aria-hidden="true">info</span>
          <p className="flex-1 font-label-sm text-label-sm text-on-surface-variant">
            {t('terminal.historyWarning')}
          </p>
          <button
            type="button"
            onClick={() => setShowHistoryHint(false)}
            aria-label={t('terminal.panel.dismissHint')}
            className="p-0.5 rounded text-on-surface-variant hover:bg-surface-container focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
          </button>
        </div>
      )}

      {/* Active terminal surface. xterm manages its own inner DOM. */}
      <div className="flex-1 min-h-0 relative">
        <div
          ref={containerRef}
          data-testid="terminal-surface"
          className="absolute inset-0 overflow-hidden px-xs py-xs"
        />
        {tabs.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center">
            <p className="font-label-md text-label-md text-on-surface-variant">
              {t('terminal.empty')}
            </p>
          </div>
        )}
      </div>
    </section>
  );
}
