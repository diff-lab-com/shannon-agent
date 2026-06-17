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
const MissionControl = lazy(() => import('./pages/MissionControl'));
const Triage = lazy(() => import('./pages/Triage'));
const Goals = lazy(() => import('./pages/Goals'));
const Routines = lazy(() => import('./pages/Routines'));
const Hooks = lazy(() => import('./pages/Hooks'));
const Profiles = lazy(() => import('./pages/Profiles'));
const Extensions = lazy(() => import('./pages/Extensions'));
const Settings = lazy(() => import('./pages/Settings'));
const OPC = lazy(() => import('./pages/OPC'));
const OPCTask = lazy(() => import('./pages/OPCTask'));
const QuickFix = lazy(() => import('./pages/QuickFix'));
const Editor = lazy(() => import('./pages/Editor'));
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
                <Route path="/mission-control" element={<MissionControl />} />
                <Route path="/triage" element={<Triage />} />
                <Route path="/goals" element={<Goals />} />
                <Route path="/routines" element={<Routines />} />
                <Route path="/hooks" element={<Hooks />} />
                <Route path="/profiles" element={<Profiles />} />
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
                <Route path="/quickfix" element={<QuickFix />} />
                <Route path="/editor" element={<Editor />} />
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
