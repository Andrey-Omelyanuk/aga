import { lazy, Suspense } from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom';
import NotFoundPage from './pages/404';
import './index.css';

const AppLayout = lazy(() => import('./pages/app/layout'));
const ProjectsPage = lazy(() => import('./pages/app/projects'));
const AgentSetsPage = lazy(() => import('./pages/app/agentSets'));
const CapabilitiesPage = lazy(() => import('./pages/app/capabilities'));
const CapabilityHistoryPage = lazy(() => import('./pages/app/capabilityHistory'));
const WorkstationsPage = lazy(() => import('./pages/app/workstations'));
const SessionsPage = lazy(() => import('./pages/app/sessions'));
const FilesPage = lazy(() => import('./pages/app/files'));
const ChatPage = lazy(() => import('./pages/app/chat'));
const SettingsPage = lazy(() => import('./pages/app/settings'));
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
          <Route path="agent-sets" element={<AgentSetsPage />} />
          <Route path="capabilities" element={<CapabilitiesPage />} />
          <Route path="skills/:id/history" element={<CapabilityHistoryPage />} />
          <Route path="commands/:id/history" element={<CapabilityHistoryPage />} />
          <Route path="workstations" element={<WorkstationsPage />} />
          <Route path="sessions" element={<SessionsPage />} />
          <Route path="files" element={<FilesPage />} />
          <Route path="chat" element={<ChatPage />} />
          <Route path="chat/:id" element={<ChatPage />} />
          <Route path="settings" element={<SettingsPage />} />
          <Route path="profile" element={<ProfilePage />} />
          <Route path="*" element={<Navigate to="/projects" replace />} />
        </Route>
        <Route path="*" element={<NotFoundPage />} />
      </Routes>
    </BrowserRouter>,
);