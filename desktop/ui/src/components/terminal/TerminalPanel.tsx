/**
 * P1-5 D — integrated terminal panel (chat page bottom drawer).
 *
 * xterm.js renders the PTY byte stream the Rust pump streams over the
 * `terminal:output` event (base64 → decoded here, ≤16 ms coalesced). All
 * process operations go through the frozen `terminal_*` commands; this
 * component never touches a shell surface itself.
 *
 * UX contract (brief):
 *  - bottom drawer, full-height toggle; NO free-form drag layout (T11);
 *  - persisted settings (`terminal_get_settings`, P3-1) are fetched once
 *    on first open: they set the drawer's initial height and the options
 *    of every xterm created this session — live instances are never
 *    re-geometried;
 *  - tabs are scoped to the panel's project (US7): the chat page passes
 *    the session's working dir and the tablist shows only matching
 *    terminals (raw spawn-request dir first, via the additive
 *    `projectDirRaw`, then the backend-canonicalized `projectDir`).
 *    Hidden tabs KEEP RUNNING — the ≤4 cap is global on the
 *    backend, so the filter is view-only;
 *  - Ctrl+` toggles the panel — registered here (not in the global
 *    useKeyboardShortcuts map) because xterm's hidden textarea would be
 *    skipped by that hook's INPUT/TEXTAREA guard. Checked against the
 *    registered global shortcuts (Ctrl+Shift+S/N/K) and the in-app map
 *    (mod+n/k/d/1-6, ?, /): Ctrl+` was free;
 *  - theme follows the app theme registry (live tokens, dark/light ANSI
 *    floor — see ./xtermTheme);
 *  - reconnect: `terminal_list` restores live tabs and `terminal_history`
 *    (US6) replays each tab's scrollback silently — the old "history is
 *    gone" warning banner is retired with it;
 *  - chat integration (US4, direction A — user-initiated only): the
 *    `shannon:terminal-run` window event runs a chat code block here, and
 *    the toolbar's "send to agent" hands a selection to the composer via
 *    `shannon:composer-prefill`;
 *  - a11y: labelled region, tablist semantics, focus moves into the
 *    terminal on open and back to the toggle button on close.
 */
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { useIntl, type PrimitiveType } from 'react-intl';
import { toastError } from '@/lib/errorToast';
import '@xterm/xterm/css/xterm.css';
import * as api from '@/lib/tauri-api';
import type { TerminalInfo, TerminalSettings } from '@/types';
import { decodeTerminalOutput, listenTerminalExit, listenTerminalOutput } from '@/lib/runtime/terminalEvents';
import { xtermTheme } from './xtermTheme';
import { readResolvedThemeAttr, useResolvedThemeAttr } from '@/hooks/useResolvedThemeAttr';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import type { Terminal as XTerm } from '@xterm/xterm';
import type { FitAddon as XTermFitAddon } from '@xterm/addon-fit';

/**
 * Hook form lives in `@/hooks/useResolvedThemeAttr` (shared with the
 * editor's CodeMirror theme) — see there for why the attribute is read
 * instead of `useTheme()`.
 */

/** Backend cap (terminal_commands.rs MAX_TERMINALS) mirrored for the UI.
 *  GLOBAL across projects — the per-project tab filter below is view-only
 *  and must not change it. */
const MAX_TERMINALS = 4;

/** Fallback drawer height (brief: ~320px) until persisted settings load. */
const DRAWER_HEIGHT_PX = 320;

/**
 * Window CustomEvent from the chat code-block "run in terminal" action
 * (components/chat/Markdown), payload `{ code: string }`. String literals
 * are the established cross-component idiom here (cf. `shannon:open-editor`);
 * the dispatch side carries the matching literal.
 */
const TERMINAL_RUN_EVENT = 'shannon:terminal-run';

/**
 * Window CustomEvent consumed by the chat page (pages/Chat.tsx), payload
 * `{ text: string }` — the composer prefill carrying the terminal
 * selection quoted as a fenced block (US4, "send to agent").
 */
const COMPOSER_PREFILL_EVENT = 'shannon:composer-prefill';

/** xterm defaults matching the backend's `[terminal]` fallbacks. */
const DEFAULT_FONT_SIZE = 12;
const DEFAULT_SCROLLBACK = 5000;

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
  /**
   * US6 replay: false until this tab's `terminal_history` snapshot has
   * been written to the term. Live payloads arriving before that are
   * queued in `pendingLive` (arrival order) and flushed after the
   * snapshot, so ordering is preserved without gaps or duplicates. The
   * fetch happens once — ensureTerm never runs twice for one entry.
   */
  historyReady: boolean;
  pendingLive: Uint8Array[];
}

/** basename(3)-ish label so tabs read "shannon-lane-f" not "/home/…". */
function dirLabel(projectDir: string): string {
  const trimmed = projectDir.replace(/\/+$/, '');
  const base = trimmed.split('/').pop() ?? trimmed;
  return base.length > 0 ? base : projectDir;
}

/**
 * US7 (P3-12): exact-match a tab against the panel's project. A null or
 * empty `projectDir` prop shows ALL terminals — same as the pre-filter
 * panel.
 *
 * Review fix: the backend canonicalizes the spawn dir before storing it
 * (`projectDir`), so the RAW prop is matched against the additive
 * `projectDirRaw` first and only falls back to the canonical field — a
 * symlinked path segment on Unix (or a `\\?\C:\…` verbatim prefix on
 * Windows) otherwise makes the freshly spawned tab vanish into the empty
 * state.
 */
function isForProject(info: TerminalInfo, projectDir?: string | null): boolean {
  if (!projectDir) return true;
  return (info.projectDirRaw ?? info.projectDir) === projectDir;
}

export interface TerminalPanelProps {
  projectDir?: string | null
}

export function TerminalPanel({ projectDir }: TerminalPanelProps) {
  const intl = useIntl();
  const t = useCallback(
    (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values),
    [intl],
  );
  const resolvedTheme = useResolvedThemeAttr();

  const [open, setOpen] = useState(false);
  const [fullHeight, setFullHeight] = useState(false);
  // P3-1: initial height comes from the persisted `drawerHeight` once the
  // boot fetch resolves; until then the brief's ~320px fallback applies.
  const [drawerHeight, setDrawerHeight] = useState(DRAWER_HEIGHT_PX);
  const [tabs, setTabs] = useState<TerminalTab[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [booted, setBooted] = useState(false);
  // P2 (review §5): a multi-line paste into a shell executes every line —
  // intercept pastes containing line breaks behind an explicit confirmation.
  const [pendingPaste, setPendingPaste] = useState<string | null>(null);
  // P2-3: the + button stays clickable at the terminal cap (aria-disabled,
  // not disabled) so clicking it can trigger the live-region cap feedback
  // below — a disabled button can neither explain nor be asked why.
  const [capFeedback, setCapFeedback] = useState(false);
  // US4 "send to agent": whether the ACTIVE xterm holds a non-empty
  // selection (drives the toolbar button below). xterm has no selection
  // change → React binding, so the onSelectionChange callback in
  // ensureTerm maintains it for the active tab only.
  const [hasSelection, setHasSelection] = useState(false);

  const containerRef = useRef<HTMLDivElement>(null);
  const toggleRef = useRef<HTMLButtonElement>(null);
  const termsRef = useRef<Map<string, TermEntry>>(new Map());
  // P3-1: settings read at xterm creation time. Mirrored through a ref so
  // the stable `ensureTerm` closure always sees the freshly booted values
  // without re-subscribing terminals.
  const settingsRef = useRef<TerminalSettings | null>(null);
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

  // US7 (P3-12): the tablist only shows THIS project's terminals. The
  // filter is deliberately view-only — hidden tabs keep running and still
  // count against the global ≤4 backend cap (MAX_TERMINALS above); the +
  // button's enabled state therefore consults `tabs`, not this list.
  const visibleTabs = useMemo(
    () => tabs.filter(tab => isForProject(tab.info, projectDir)),
    [tabs, projectDir],
  );

  // The active tab only counts when it is visible here: a projectDir prop
  // change (session switch) can orphan the previous activeId on a hidden
  // tab, and the drawer then shows the empty state rather than rendering a
  // terminal its tablist doesn't list.
  const activeTab = useMemo(
    () => visibleTabs.find(tab => tab.info.terminalId === activeId) ?? null,
    [visibleTabs, activeId],
  );

  // Latest active id for the onSelectionChange callback registered inside
  // the stable ensureTerm closure: the toolbar's selection state tracks
  // the ACTIVE tab only, so background tabs with a selection must not
  // enable the button.
  const activeIdRef = useRef<string | null>(activeId);
  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);

  // Re-derive the toolbar selection state when the active tab changes —
  // switching to a tab that holds a selection must enable "send to agent"
  // even though no selection event fires on the switch itself.
  useEffect(() => {
    const entry = activeId ? termsRef.current.get(activeId) : undefined;
    setHasSelection((entry?.term.getSelection() ?? '').length > 0);
  }, [activeId]);

  // ── xterm instance lifecycle ────────────────────────────────────────

  const ensureTerm = useCallback((info: TerminalInfo): TermEntry | null => {
    const existing = termsRef.current.get(info.terminalId);
    if (existing) return existing;
    const xterm = xtermModuleRef.current;
    if (!xterm) return null; // module still loading — next effect run mounts

    const fit = new xterm.FitAddon();
    // P3-1: persisted preferences apply at CREATION only — never mutate a
    // live instance's geometry (font/scrollback changes land in terminals
    // opened afterwards).
    const settings = settingsRef.current;
    const term = new xterm.Terminal({
      convertEol: false,
      cursorBlink: true,
      fontFamily: FONT_FAMILY,
      fontSize: settings?.fontSize ?? DEFAULT_FONT_SIZE,
      scrollback: settings?.scrollback ?? DEFAULT_SCROLLBACK,
      screenReaderMode: settings?.screenReaderMode ?? false,
      // P1-36: read the CURRENT theme off `<html data-theme>` at creation —
      // this callback has stable deps, so the `resolvedTheme` captured by
      // the closure is stale for any instance created after a theme switch.
      // (Live instances are still re-themed wholesale by the effect below.)
      theme: xtermTheme(readResolvedThemeAttr()),
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
    // US4: keep the "send to agent" button honest for the ACTIVE tab.
    // xterm's onSelectionChange carries no payload — read the selection
    // off the instance (empty string once cleared).
    term.onSelectionChange(() => {
      if (activeIdRef.current !== info.terminalId) return;
      setHasSelection(term.getSelection().length > 0);
    });

    // Created BEFORE the subscriptions: the output handler consults the
    // entry's replay-queue state (US6) on every live payload.
    const entry: TermEntry = {
      term,
      fit,
      detachOutput: () => {}, // wired below, once the subscriptions exist
      historyReady: false,
      pendingLive: [],
    };

    // Both subscriptions resolve asynchronously (Tauri listen); a disposed
    // entry unsubscribes immediately on resolution so an
    // unmount-before-subscribe race never leaks a listener.
    let disposed = false;
    const unsubs: Array<() => void> = [];
    void listenTerminalOutput(payload => {
      if (payload.terminalId !== info.terminalId) return;
      // Raw bytes go straight to xterm: the pump slices the pty stream at
      // arbitrary byte boundaries, and xterm's write buffer completes
      // multi-byte sequences split across events. Decoding per event would
      // turn both halves of a split sequence into U+FFFD.
      const bytes = decodeTerminalOutput(payload.data);
      // US6: until the history snapshot has been written, live bytes queue
      // in arrival order — the backend appends to the replay ring BEFORE
      // emitting, so the snapshot is a clean prefix of the live stream and
      // snapshot-then-flush reproduces the true order without duplicates.
      if (!entry.historyReady) {
        entry.pendingLive.push(bytes);
        return;
      }
      term.write(bytes);
    }).then(fn => {
      if (disposed) {
        fn();
        return;
      }
      unsubs.push(fn);
    });
    // P3-6: exit is signaled by the dedicated `terminal:exit` event, never
    // by parsing the in-stream "[shannon: process exited …" notice (that
    // text is for humans — any program could print it). The backend
    // emission lands with the Task-4 pump change; until then the tab just
    // never auto-marks exited.
    void listenTerminalExit(payload => {
      if (payload.terminalId !== info.terminalId) return;
      setTabs(prev => prev.map(tab => (
        tab.info.terminalId === info.terminalId ? { ...tab, exited: true } : tab
      )));
    }).then(fn => {
      if (disposed) {
        fn();
        return;
      }
      unsubs.push(fn);
    });
    entry.detachOutput = () => {
      disposed = true;
      unsubs.forEach(fn => fn());
    };

    termsRef.current.set(info.terminalId, entry);

    // US6: replay the pre-reconnect scrollback once per tab (ensureTerm
    // never builds a second entry for the same id). The payload is base64
    // of the ring's bytes; empty (unknown id / ended session / mock
    // backend) and failed calls simply restore nothing — the live stream
    // continues either way and no banner is shown anymore.
    void (async () => {
      let history: { data: string } | null = null;
      try {
        history = await api.terminalHistory(info.terminalId);
      } catch {
        /* no replay backend — live stream only */
      }
      if (disposed) return;
      if (history && history.data) term.write(decodeTerminalOutput(history.data));
      const queued = entry.pendingLive;
      entry.pendingLive = [];
      entry.historyReady = true;
      queued.forEach(bytes => term.write(bytes));
      term.scrollToBottom();
    })();

    return entry;
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
    try {
      const { terminalId } = await api.terminalSpawn(dir ?? projectDir ?? null);
      const requestedDir = dir ?? projectDir ?? '';
      const info: TerminalInfo = {
        terminalId,
        projectDir: requestedDir,
        // The optimistic placeholder carries the RAW (uncanonicalized)
        // form in both fields so the per-project filter matches until the
        // authoritative terminal_list merge below replaces it with the
        // backend's canonical + raw pair (review fix).
        projectDirRaw: requestedDir,
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
    } catch (e) {
      // P2: spawn used to be an unhandled rejection — surface it instead.
      // Swallowed here so both call sites (`void spawnTab()` on the +
      // button, `await spawnTab()` in openPanel) stay rejection-free.
      toastError(t('terminal.spawn.failed'), e);
      return undefined;
    }
  }, [projectDir, t]);

  /**
   * Open the drawer; first open loads settings and reconciles with the
   * backend. Resolves with the terminal the drawer ended up on (the
   * newest tab visible in THIS project — existing or freshly spawned) so
   * callers like the chat "run in terminal" event never race the
   * re-render; null when no terminal could be selected or spawned (the
   * failure is already toasted by spawnTab).
   */
  const openPanel = useCallback(async (): Promise<string | null> => {
    setOpen(true);
    if (booted) {
      // Re-open with preserved state: select the newest tab visible in
      // this project (the previous activeId may have been orphaned on a
      // hidden tab by a projectDir prop change).
      const visible = tabs.filter(tab => isForProject(tab.info, projectDir));
      if (visible.length === 0) return null;
      const selected = visible[visible.length - 1].info.terminalId;
      if (selected !== activeId) setActiveId(selected);
      return selected;
    }
    setBooted(true);
    // P3-1: fetch persisted settings BEFORE any terminal is created, so
    // the drawer's initial height and the first xterm instance already
    // honor them. Best-effort: backend defaults (or a failed call) leave
    // the built-in fallbacks in place.
    try {
      const settings = await api.terminalGetSettings();
      settingsRef.current = settings;
      setDrawerHeight(settings.drawerHeight);
    } catch {
      // No settings (plain-browser dev / backend down): keep defaults.
    }
    try {
      const list = await api.terminalList();
      const known = list.slice(0, MAX_TERMINALS).map(info => ({ info, exited: false }));
      setTabs(known);
      const visible = known.filter(tab => isForProject(tab.info, projectDir));
      if (visible.length > 0) {
        // Newest listed terminal wins (backend ordering is oldest-first).
        const selected = visible[visible.length - 1].info.terminalId;
        setActiveId(selected);
        return selected;
      }
      // US7: every live terminal may belong to another project. Only a
      // genuinely empty backend spawns a first tab here — a filter that
      // hides everything shows the empty state instead of auto-spawning.
      if (known.length === 0) return (await spawnTab()) ?? null;
      return null;
    } catch {
      // Backend unreachable: the drawer still opens and shows the error
      // state via the empty tab list (retry through the + button).
      return null;
    }
  }, [booted, activeId, tabs, projectDir, spawnTab]);

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
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key === '`') {
        e.preventDefault();
        e.stopPropagation();
        togglePanel();
      }
    };
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [togglePanel]);

  // ── Chat integration: "run in terminal" (US4, direction A) ──────────
  // Chat fenced code blocks dispatch the `shannon:terminal-run` window
  // CustomEvent with `{ code }` (components/chat/Markdown). The panel owns
  // its open/spawn state (drawer-only since Task 3), so this event is the
  // seam: a closed drawer opens exactly like the toggle (openPanel resolves
  // with the tab it selected/spawned), a drawer with no tabs spawns exactly
  // like the + button, and the code lands on the ACTIVE terminal with a
  // trailing newline (the shell executes on Enter — without it the command
  // would sit at the prompt). An existing tab is never spawned twice.
  const runInTerminal = useCallback(async (code: string) => {
    // Every state read below happens BEFORE the first await — no stale
    // closure and no render race with the boot sequence. The active tab is
    // only a target when it is visible in this project (US7): after a
    // session switch the stale activeId may sit on a hidden tab.
    let target: string | null;
    if (open) {
      const currentVisible = activeId != null
        && visibleTabs.some(tab => tab.info.terminalId === activeId);
      target = currentVisible ? activeId : ((await spawnTab()) ?? null);
    } else {
      target = await openPanel();
    }
    if (!target) return; // spawn failed — spawnTab already toasted
    try {
      await api.terminalWrite(target, `${code}\n`);
    } catch {
      /* dead terminal: nothing sensible to run code into */
    }
  }, [open, activeId, visibleTabs, openPanel, spawnTab]);

  useEffect(() => {
    const onRun = (e: Event) => {
      const code = (e as CustomEvent<{ code?: unknown }>).detail?.code;
      if (typeof code !== 'string' || code.length === 0) return;
      void runInTerminal(code);
    };
    window.addEventListener(TERMINAL_RUN_EVENT, onRun);
    return () => window.removeEventListener(TERMINAL_RUN_EVENT, onRun);
  }, [runInTerminal]);

  // ── Multi-line paste guard (P2, review §5) ──────────────────────────
  // A paste containing line breaks makes the shell execute every line —
  // pasting a block of `rm …` or `git push …` lines into the wrong window
  // is a classic footgun. Intercepted on the container in the CAPTURE
  // phase (an ancestor of xterm's hidden textarea) so xterm's own paste
  // handler never sees the multi-line event. Single-line pastes are left
  // untouched and keep xterm's native path (incl. bracketed paste).

  const confirmPaste = useCallback(() => {
    const text = pendingPaste;
    setPendingPaste(null);
    if (!text || !activeId) return;
    void api.terminalWrite(activeId, text).catch(() => {
      /* dead terminal: nothing sensible to report a paste into */
    });
  }, [pendingPaste, activeId]);

  useEffect(() => {
    const container = containerRef.current;
    if (!open || !container) return;
    const onPaste = (e: ClipboardEvent) => {
      const text = e.clipboardData?.getData('text/plain') ?? '';
      if (!text || !(text.includes('\n') || text.includes('\r'))) return;
      e.preventDefault();
      e.stopPropagation();
      setPendingPaste(text);
    };
    container.addEventListener('paste', onPaste, true);
    return () => container.removeEventListener('paste', onPaste, true);
  }, [open, activeId]);

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
        // US7: the successor is the newest tab VISIBLE in this project —
        // hidden tabs must not become the active one.
        const visible = next.filter(tab => isForProject(tab.info, projectDir));
        return visible.length > 0 ? visible[visible.length - 1].info.terminalId : null;
      });
      return next;
    });
  }, [projectDir]);

  const canSpawn = tabs.length < MAX_TERMINALS;

  // ── Tablist a11y (P2-3: WAI-ARIA tabs pattern) ───────────────────────
  // Roving tabindex: only the ACTIVE tab is in the tab order; the arrow
  // keys move the selection (wrapping) and Home/End jump to the first/
  // last tab. Selection moves with the key, then focus follows onto the
  // newly selected tab button.

  const tabButtonRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  const onTablistKeyDown = useCallback((e: ReactKeyboardEvent) => {
    if (visibleTabs.length === 0) return;
    const ids = visibleTabs.map(tab => tab.info.terminalId);
    const current = activeId ? ids.indexOf(activeId) : -1;
    let nextId: string | null = null;
    if (e.key === 'ArrowRight') nextId = ids[(current + 1) % ids.length];
    else if (e.key === 'ArrowLeft') nextId = ids[(current - 1 + ids.length) % ids.length];
    else if (e.key === 'Home') nextId = ids[0];
    else if (e.key === 'End') nextId = ids[ids.length - 1];
    if (!nextId) return;
    e.preventDefault();
    setActiveId(nextId);
    // `const` binding so the rAF closure keeps the narrowed type.
    const target: string = nextId;
    requestAnimationFrame(() => tabButtonRefs.current.get(target)?.focus());
  }, [visibleTabs, activeId]);

  const handleSpawnClick = useCallback(() => {
    if (!canSpawn) {
      setCapFeedback(true);
      return;
    }
    void spawnTab();
  }, [canSpawn, spawnTab]);

  // US4 (second half): hand the active terminal's selection to the chat
  // composer as a fenced (quoted) block. The drawer stays open — Chat.tsx
  // prefills the draft and re-focuses the composer via the established
  // `shannon:focus-composer` event, so typing continues below the block.
  const handleSendToAgent = useCallback(() => {
    if (!activeId) return;
    const selection = termsRef.current.get(activeId)?.term.getSelection() ?? '';
    if (selection.length === 0) return;
    window.dispatchEvent(new CustomEvent(COMPOSER_PREFILL_EVENT, {
      detail: { text: `\`\`\`\n${selection}\n\`\`\`` },
    }));
  }, [activeId]);

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
      className={`flex flex-col bg-surface-container-lowest shrink-0 border-t border-outline-variant/30 ${
        fullHeight ? 'flex-1 min-h-0' : ''
      }`}
      style={!fullHeight ? { height: drawerHeight } : undefined}
    >
      {/* Toolbar: tabs + actions */}
      <div className="flex items-center gap-xs px-sm py-1 border-b border-outline-variant/20 bg-surface-container-low/60">
        <div
          role="tablist"
          aria-label={t('terminal.panel.label')}
          onKeyDown={onTablistKeyDown}
          className="flex items-center gap-xs flex-1 min-w-0 overflow-x-auto"
        >
          {visibleTabs.map(tab => {
            const selected = tab.info.terminalId === activeId;
            return (
              <div key={tab.info.terminalId} className="flex items-center shrink-0">
                <button
                  ref={el => {
                    if (el) tabButtonRefs.current.set(tab.info.terminalId, el);
                    else tabButtonRefs.current.delete(tab.info.terminalId);
                  }}
                  id={`terminal-tab-${tab.info.terminalId}`}
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  aria-controls="terminal-tab-panel"
                  tabIndex={selected ? 0 : -1}
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
          onClick={handleSpawnClick}
          aria-disabled={!canSpawn}
          aria-label={t('terminal.tab.new')}
          title={canSpawn ? t('terminal.tab.new') : t('terminal.maxReached', { max: MAX_TERMINALS })}
          className="p-1 rounded text-on-surface-variant hover:bg-surface-container disabled:opacity-40 disabled:cursor-not-allowed aria-disabled:opacity-40 aria-disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">add</span>
        </button>
        {/* P3-4: cap feedback is announced, not just painted on the title —
            the text swaps in on a capped + click and the live region (plus
            the change) is picked up by screen readers. */}
        <span aria-live="polite" className="sr-only">
          {capFeedback ? t('terminal.maxReached', { max: MAX_TERMINALS }) : ''}
        </span>
        {/* US4: send the active terminal's selection to the chat composer.
            Enabled only while a selection exists (xterm has no selection →
            React binding, so the state is maintained by the panel). */}
        <button
          type="button"
          onClick={handleSendToAgent}
          disabled={!hasSelection}
          aria-label={t('terminal.sendToAgent.title')}
          title={t('terminal.sendToAgent.title')}
          className="p-1 rounded text-on-surface-variant hover:bg-surface-container disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">send</span>
        </button>
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

      {/* Active terminal surface — the tabpanel the tabs control (P2-3:
          aria-labelledby ↔ the active tab's aria-controls). xterm manages
          its own inner DOM. */}
      <div className="flex-1 min-h-0 relative">
        <div
          ref={containerRef}
          data-testid="terminal-surface"
          role="tabpanel"
          id="terminal-tab-panel"
          aria-labelledby={activeTab ? `terminal-tab-${activeTab.info.terminalId}` : undefined}
          className="absolute inset-0 overflow-hidden px-xs py-xs"
        />
        {/* US7: the empty state also covers "terminals exist but none for
            this project" — deliberately NOT auto-spawning one. */}
        {visibleTabs.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center">
            <p className="font-label-md text-label-md text-on-surface-variant">
              {t('terminal.empty')}
            </p>
          </div>
        )}
      </div>

      {/* Multi-line paste confirmation (P2, review §5). */}
      <ConfirmDialog
        open={pendingPaste !== null}
        title={t('terminal.paste.multiline.title')}
        message={t('terminal.paste.multiline.message', {
          lines: pendingPaste ? pendingPaste.split(/\r\n|\r|\n/).length : 0,
        })}
        confirmLabel={t('terminal.paste.multiline.confirm')}
        cancelLabel={t('terminal.paste.multiline.cancel')}
        onConfirm={confirmPaste}
        onCancel={() => setPendingPaste(null)}
      />
    </section>
  );
}
