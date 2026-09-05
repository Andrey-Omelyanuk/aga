import { lazy, Suspense } from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom';
import NotFoundPage from './pages/404';
import './index.css';

const AppLayout = lazy(() => import('./pages/app/layout'));
const ProjectsPage = lazy(() => import('./pages/app/projects'));
const AgentSetsPage = lazy(() => import('./pages/app/agentSets'));
const ConfigCapabilitiesPage = lazy(() => import('./pages/app/configCapabilities'));
const ConfigEnvPage = lazy(() => import('./pages/app/configEnv'));
const ConfigUsersPage = lazy(() => import('./pages/app/configUsers'));
const ConfigLlmPage = lazy(() => import('./pages/app/configLlm'));
const ConfigHelpPage = lazy(() => import('./pages/app/configHelp'));
const CapabilityHistoryPage = lazy(() => import('./pages/app/capabilityHistory'));
const WorkstationsPage = lazy(() => import('./pages/app/workstations'));
const SessionsPage = lazy(() => import('./pages/app/sessions'));
const FilesPage = lazy(() => import('./pages/app/files'));
const ChangesPage = lazy(() => import('./pages/app/changes'));
const ChatPage = lazy(() => import('./pages/app/chat'));
const ProfilePage = lazy(() => import('./pages/app/profile'));

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <BrowserRouter>
      <Routes>
        <Route
          path="/"
          element={
            <Suspense fallback={<div className="p-10 text-slate-400">Загрузка…</div>}>
              <AppLayout />
            </Suspense>
          }
        >
          <Route index element={<Navigate to="/projects" replace />} />
          <Route path="projects" element={<ProjectsPage />} />
          <Route path="config/env" element={<ConfigEnvPage />} />
          <Route path="config/users" element={<ConfigUsersPage />} />
          <Route path="config/skills" element={<ConfigCapabilitiesPage />} />
          <Route path="config/commands" element={<ConfigCapabilitiesPage />} />
          <Route path="config/agent-sets" element={<AgentSetsPage />} />
          <Route path="config/llms" element={<ConfigLlmPage />} />
          <Route path="config/help" element={<ConfigHelpPage />} />
          <Route path="skills/:id/history" element={<CapabilityHistoryPage />} />
          <Route path="commands/:id/history" element={<CapabilityHistoryPage />} />
          <Route path="workstations" element={<WorkstationsPage />} />
          <Route path="sessions" element={<SessionsPage />} />
          <Route path="files" element={<FilesPage />} />
          <Route path="workstations/:id/changes" element={<ChangesPage />} />
          <Route path="chat" element={<ChatPage />} />
          <Route path="chat/:id" element={<ChatPage />} />
          <Route path="profile" element={<ProfilePage />} />
          <Route path="settings" element={<Navigate to="/config/env" replace />} />
          <Route path="agent-sets" element={<Navigate to="/config/agent-sets" replace />} />
          <Route path="capabilities" element={<Navigate to="/config/skills" replace />} />
          <Route path="*" element={<Navigate to="/projects" replace />} />
        </Route>
        <Route path="*" element={<NotFoundPage />} />
      </Routes>
    </BrowserRouter>,
);