import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { Layout } from './components/Layout';
import Chat from './pages/Chat';
import Tasks from './pages/Tasks';
import Goals from './pages/Goals';
import Extensions from './pages/Extensions';
import Settings from './pages/Settings';
import OPC from './pages/OPC';
import OPCTask from './pages/OPCTask';
import ExtensionsHub from './components/extensions/ExtensionsHub';
import MyAgents from './components/extensions/MyAgents';
import DataSources from './components/extensions/DataSources';

import GeneralSettings from './components/settings/GeneralSettings';
import ThemeSettings from './components/settings/ThemeSettings';
import ModelsSettings from './components/settings/ModelsSettings';
import AdvancedSettings from './components/settings/AdvancedSettings';
import BillingSettings from './components/settings/BillingSettings';

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route element={<Layout />}>
          <Route path="/" element={<Navigate to="/chat" replace />} />
          <Route path="/chat" element={<Chat />} />
          <Route path="/tasks" element={<Tasks />} />
          <Route path="/goals" element={<Goals />} />
          <Route path="/extensions" element={<Extensions />}>
            <Route index element={<Navigate to="skills" replace />} />
            <Route path="skills" element={<ExtensionsHub />} />
            <Route path="agents" element={<MyAgents />} />
            <Route path="datasources" element={<DataSources />} />
          </Route>
          <Route path="/opc" element={<OPC />} />
          <Route path="/opc/task" element={<OPCTask />} />
          <Route path="/settings" element={<Settings />}>
            <Route index element={<Navigate to="general" replace />} />
            <Route path="general" element={<GeneralSettings />} />
            <Route path="theme" element={<ThemeSettings />} />
            <Route path="models" element={<ModelsSettings />} />
            <Route path="billing" element={<BillingSettings />} />
            <Route path="advanced" element={<AdvancedSettings />} />
          </Route>
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
