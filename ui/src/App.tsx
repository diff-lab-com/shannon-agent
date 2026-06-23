import { lazy, Suspense } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { Toaster } from 'sonner';
import { AppProvider } from './context/AppContext';
import { ThemeProvider } from './context/ThemeContext';
import { I18nProvider } from './i18n';
import { ErrorBoundary } from './components/ErrorBoundary';
import { Layout } from './components/Layout';

const Welcome = lazy(() => import('./pages/Welcome'));
const Chat = lazy(() => import('./pages/Chat'));
const Tasks = lazy(() => import('./pages/Tasks'));
const Triage = lazy(() => import('./pages/Triage'));
const Extensions = lazy(() => import('./pages/Extensions'));
const Settings = lazy(() => import('./pages/Settings'));
const OPC = lazy(() => import('./pages/OPC'));
const OPCTask = lazy(() => import('./pages/OPCTask'));
const QuickFix = lazy(() => import('./pages/QuickFix'));
const Editor = lazy(() => import('./pages/Editor'));
const Memory = lazy(() => import('./pages/Memory'));
const DataSources = lazy(() => import('./components/extensions/DataSources'));
const Featured = lazy(() => import('./components/extensions/Featured'));
const McpServers = lazy(() => import('./components/extensions/McpServers'));
const Skills = lazy(() => import('./components/extensions/Skills'));
const Agents = lazy(() => import('./components/extensions/Agents'));
const Plugins = lazy(() => import('./components/extensions/Plugins'));
const Installed = lazy(() => import('./components/extensions/Installed'));
const GeneralSettings = lazy(() => import('./components/settings/GeneralSettings'));
const ThemeSettings = lazy(() => import('./components/settings/ThemeSettings'));
const ModelsSettings = lazy(() => import('./components/settings/ModelsSettings'));
const AdvancedSettings = lazy(() => import('./components/settings/AdvancedSettings'));
const BillingSettings = lazy(() => import('./components/settings/BillingSettings'));
const NotificationsSettings = lazy(() => import('./components/settings/NotificationsSettings'));

function PageLoader() {
  return <div className="flex-1 flex items-center justify-center"><span className="material-symbols-outlined text-[32px] text-primary animate-spin">progress_activity</span></div>;
}

export default function App() {
  return (
    <I18nProvider>
    <ThemeProvider>
      <AppProvider>
        <ErrorBoundary>
        <BrowserRouter>
          <Suspense fallback={<PageLoader />}>
            <Routes>
              <Route path="/welcome" element={<Welcome />} />
              <Route element={<Layout />}>
                <Route path="/" element={<Navigate to="/chat" replace />} />
                {/* Legacy route redirects — keep old bookmarks/links working. */}
                <Route path="/strategic-focus" element={<Navigate to="/opc" replace />} />
                <Route path="/agent-swarm" element={<Navigate to="/opc" replace />} />
                <Route path="/quick-inject" element={<Navigate to="/tasks" replace />} />
                <Route path="/background-tasks" element={<Navigate to="/tasks" replace />} />
                <Route path="/chat" element={<Chat />} />
                <Route path="/tasks" element={<Tasks />} />
                <Route path="/triage" element={<Triage />} />
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
                </Route>
                <Route path="/opc" element={<OPC />} />
                <Route path="/opc/task" element={<OPCTask />} />
                <Route path="/opc/task/:id" element={<OPCTask />} />
                <Route path="/quickfix" element={<QuickFix />} />
                <Route path="/editor" element={<Editor />} />
                <Route path="/memory" element={<Memory />} />
                <Route path="/settings" element={<Settings />}>
                  <Route index element={<Navigate to="general" replace />} />
                  <Route path="general" element={<GeneralSettings />} />
                  <Route path="theme" element={<ThemeSettings />} />
                  <Route path="models" element={<ModelsSettings />} />
                  <Route path="billing" element={<BillingSettings />} />
                  <Route path="advanced" element={<AdvancedSettings />} />
                  <Route path="notifications" element={<NotificationsSettings />} />
                </Route>
                <Route path="*" element={<Navigate to="/chat" replace />} />
              </Route>
            </Routes>
          </Suspense>
        <Toaster position="bottom-right" richColors closeButton theme="system" />
        </BrowserRouter>
        </ErrorBoundary>
      </AppProvider>
    </ThemeProvider>
    </I18nProvider>
  );
}
