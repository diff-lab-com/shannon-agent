import { lazy, Suspense, useEffect, useRef } from 'react';
import { BrowserRouter, Routes, Route, Navigate, useNavigate, useLocation } from 'react-router-dom';
import { Toaster } from 'sonner';
import { AppProvider } from './context/AppContext';
import { ThemeProvider, useTheme, themeModeOf } from './context/ThemeContext';
import { I18nProvider } from './i18n';
import { ArtifactProvider } from './components/artifact/ArtifactContext';
import { ArtifactLinkHost } from './components/artifact/ArtifactLinkHost';
import { ErrorBoundary } from './components/ErrorBoundary';
import { LinkContextMenuHost } from './components/shared/LinkContextMenu';
import { Layout } from './components/Layout';
// Office Wave 3 C3 — companion Quick Capture window receiver + fallback route.
import {
  isMainWindowLocation,
  useCompanionPromptListener,
} from './lib/companionBridge';
import { pushComposerDraft } from './lib/composerBridge';

const Welcome = lazy(() => import('./pages/Welcome'));
const Chat = lazy(() => import('./pages/Chat'));
// Office Wave 3 C3 — companion Quick Capture (standalone chrome-less page:
// the companion window's whole UI, and a fallback in the main window).
const CompanionPage = lazy(() => import('./pages/CompanionPage'));
const Tasks = lazy(() => import('./pages/Tasks'));
const Triage = lazy(() => import('./pages/Triage'));
const Extensions = lazy(() => import('./pages/Extensions'));
const Settings = lazy(() => import('./pages/Settings'));
const OPC = lazy(() => import('./pages/OPC'));
const OPCTask = lazy(() => import('./pages/OPCTask'));
const Memory = lazy(() => import('./pages/Memory'));
const Usage = lazy(() => import('./pages/Usage'));
// office Wave 2 B9' — reference-style file library.
const FilesPage = lazy(() => import('./pages/FilesPage'));
// §4.14 — Turn Timeline (inside-of-a-turn visualization over the L0 log).
const TurnTimeline = lazy(() => import('./pages/TurnTimeline'));
const SkillProposalsManager = lazy(() => import('./components/skills/SkillProposalsManager'));
const DataSources = lazy(() => import('./components/extensions/DataSources'));
const Featured = lazy(() => import('./components/extensions/Featured'));
const McpServers = lazy(() => import('./components/extensions/McpServers'));
const Skills = lazy(() => import('./components/extensions/Skills'));
const Agents = lazy(() => import('./components/extensions/Agents'));
const Plugins = lazy(() => import('./components/extensions/Plugins'));
const Installed = lazy(() => import('./components/extensions/Installed'));
// IA X1: Extensions → Pending — the single skill-review surface (评审裁决 #2).
const Pending = lazy(() => import('./components/extensions/Pending'));
const GeneralSettings = lazy(() => import('./components/settings/GeneralSettings'));
const ThemeSettings = lazy(() => import('./components/settings/ThemeSettings'));
const ModelsSettings = lazy(() => import('./components/settings/ModelsSettings'));
const AdvancedSettings = lazy(() => import('./components/settings/AdvancedSettings'));
const NotificationsSettings = lazy(() => import('./components/settings/NotificationsSettings'));
const ConnectionsSettings = lazy(() => import('./components/settings/ConnectionsSettings'));
const RemotesSettings = lazy(() => import('./components/settings/RemotesSettings'));
const PermissionsSettings = lazy(() => import('./components/settings/PermissionsSettings'));

// P2-5a spike: dev-only test page for the assistant-ui runtime adapter.
// Loaded here so `Chat.tsx` (production) and its component tree stay untouched.
// Production routing never exposes this; gating happens at the route level
// below via `import.meta.env.DEV`.

// G4 (UI review 2026-09-29): the Toaster follows the APP's resolved theme
// (was theme="system" → a dark OS + light app theme rendered inverted toasts)
// and drops richColors in favor of semantic container/on-container token
// pairs per type (success=primary, error=error, warning=tertiary,
// info=secondary — every pair is AA-validated per theme by the generator).
// The glass surface itself comes from the [data-sonner-toast] block in
// index.css (sonner's injected stylesheet is unlayered, so plain utilities
// in classNames alone cannot win the cascade there).
function ThemedToaster() {
  const { resolvedTheme } = useTheme()
  return (
    <Toaster
      position="bottom-right"
      closeButton
      theme={themeModeOf(resolvedTheme)}
      toastOptions={{
        classNames: {
          toast: 'glass-overlay',
          success: 'bg-primary-container text-on-primary-container',
          error: 'bg-error-container text-on-error-container',
          warning: 'bg-tertiary-container text-on-tertiary-container',
          info: 'bg-secondary-container text-on-secondary-container',
          description: 'opacity-80',
        },
      }}
    />
  )
}

/**
 * Office Wave 3 C3 — main-window receiver for the companion Quick Capture
 * window. Cross-window leg: the companion emits
 * `shannon:companion-prompt` targeted at `main` (Tauri event — the only
 * thing that crosses webviews). In-window leg: the Wave 2 composer draft
 * bridge (`pushComposerDraft`, window CustomEvent). Trust contract intact:
 * the capture lands as a DRAFT, never auto-sent.
 *
 * Mounted app-level (inside the router) so it exists regardless of route.
 * Exported named for tests.
 */
export function CompanionPromptBridge() {
  const navigate = useNavigate();
  const location = useLocation();
  const locationRef = useRef(location);
  useEffect(() => {
    locationRef.current = location;
  });
  useCompanionPromptListener((text) => {
    // `emitTo` already scopes delivery to `main`; this guard additionally
    // keeps session windows inert if a window's boot URL is ambiguous and
    // skips the main window when IT is showing the /companion fallback page
    // (its own Send would otherwise navigate itself away mid-capture).
    if (!isMainWindowLocation()) return;
    if (locationRef.current.pathname === '/chat') {
      pushComposerDraft(text);
    } else {
      // The composer lives on /chat and is not mounted yet — navigate
      // first, then push once. Single deferred push on purpose: ChatInput
      // APPENDS drafts, so a retry loop could duplicate the text. The
      // fixed delay covers the common (already-loaded chunk) case; a cold
      // lazy-load slower than the delay can drop the draft — accepted for
      // this wave, see the C3 report.
      navigate('/chat');
      window.setTimeout(() => pushComposerDraft(text), 150);
    }
  });
  return null;
}

export default function App() {
  return (
    <I18nProvider>
    <ThemeProvider>
      <AppProvider>
        {/* Batch D4: app-scoped artifact context — Settings toggles autoOpen
            while Chat's dock consumes it, so the provider wraps both. */}
        <ArtifactProvider>
        <ErrorBoundary>
        <BrowserRouter>
          {/* P0-A: global right-click menu for external links (panel/browser). */}
          <LinkContextMenuHost />
          {/* P0-B/P1-C/P1-E: links→web tabs, file chips→artifact tabs. */}
          <ArtifactLinkHost />
          {/* Office Wave 3 C3: companion Quick Capture prompts → composer drafts. */}
          <CompanionPromptBridge />
          {/* B1-16: the route-level Suspense lives in Layout (around the
              Outlet) so lazy chunks no longer unmount the whole shell; this
              top-level boundary only exists for /welcome and stays null. */}
          <Suspense fallback={null}>
            <Routes>
              <Route path="/welcome" element={<Welcome />} />
              {/* Office Wave 3 C3 — companion Quick Capture. Standalone like
                  /welcome (no Layout chrome): this is the companion window's
                  whole UI; the main window only ever renders it as a manual
                  fallback. The CompanionPromptBridge above ignores prompts
                  while a window shows this route, so the fallback page never
                  navigates itself away mid-capture. */}
              <Route path="/companion" element={<CompanionPage />} />
              <Route element={<Layout />}>
                <Route path="/" element={<Navigate to="/chat" replace />} />
                {/* Legacy route redirects — keep old bookmarks/links working. */}
                <Route path="/strategic-focus" element={<Navigate to="/opc" replace />} />
                <Route path="/agent-swarm" element={<Navigate to="/opc" replace />} />
                <Route path="/quick-inject" element={<Navigate to="/tasks" replace />} />
                <Route path="/background-tasks" element={<Navigate to="/tasks" replace />} />
                <Route path="/chat" element={<Chat />} />
                <Route path="/files" element={<FilesPage />} />
                <Route path="/tasks" element={<Tasks />} />
                <Route path="/triage" element={<Triage />} />
                <Route path="/usage" element={<Usage />} />
                {/* P1 navigation cleanup — these pages are no longer in the
                    sidebar. Routes redirect to /tasks so existing bookmarks
                    and deep links keep working. Pages and their tests remain
                    for now; they will be absorbed into Tasks tabs in a later
                    iteration or removed once superseded. */}
                <Route path="/mission-control" element={<Navigate to="/tasks" replace />} />
                <Route path="/goals" element={<Navigate to="/tasks" replace />} />
                <Route path="/routines" element={<Navigate to="/tasks" replace />} />
                <Route path="/hooks" element={<Navigate to="/tasks" replace />} />
                <Route path="/profiles" element={<Navigate to="/tasks" replace />} />
                <Route path="/extensions" element={<Extensions />}>
                  <Route index element={<Navigate to="featured" replace />} />
                  <Route path="featured" element={<Featured />} />
                  <Route path="mcp-servers" element={<McpServers />} />
                  <Route path="skills" element={<Skills />} />
                  <Route path="agents" element={<Agents />} />
                  <Route path="datasources" element={<DataSources />} />
                  <Route path="plugins" element={<Plugins />} />
                  <Route path="installed" element={<Installed />} />
                  {/* IA X1: 待处理 — skill proposals awaiting review + errors. */}
                  <Route path="pending" element={<Pending />} />
                </Route>
                <Route path="/opc" element={<OPC />} />
                <Route path="/opc/task" element={<OPCTask />} />
                <Route path="/opc/task/:id" element={<OPCTask />} />
                {/* Editor standalone page retired (audit §3.8): file editing
                    lives in the chat-inline EditorPanel (mod+5 / palette /
                    /editor slash open it there). Deep links redirect. */}
                <Route path="/editor" element={<Navigate to="/chat" replace />} />
                <Route path="/memory" element={<Memory />} />
                <Route path="/timeline/:id" element={<TurnTimeline />} />
                <Route path="/settings" element={<Settings />}>
                  <Route index element={<Navigate to="general" replace />} />
                  <Route path="general" element={<GeneralSettings />} />
                  <Route path="theme" element={<ThemeSettings />} />
                  <Route path="models" element={<ModelsSettings />} />
                  <Route path="permissions" element={<PermissionsSettings />} />
                  <Route path="advanced" element={<AdvancedSettings />} />
                  <Route path="notifications" element={<NotificationsSettings />} />
                  <Route path="connections" element={<ConnectionsSettings />} />
                  <Route path="remotes" element={<RemotesSettings />} />
                </Route>
                <Route path="*" element={<Navigate to="/chat" replace />} />
              </Route>
            </Routes>
          </Suspense>
        <ThemedToaster />
        <Suspense fallback={null}>
          <SkillProposalsManager />
        </Suspense>
        </BrowserRouter>
        </ErrorBoundary>
        </ArtifactProvider>
      </AppProvider>
    </ThemeProvider>
    </I18nProvider>
  );
}
