import { lazy, Suspense } from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom';
import NotFoundPage from './pages/404';
import './index.css';

const AppLayout = lazy(() => import('./pages/app/layout'));
const ProjectsPage = lazy(() => import('./pages/app/projects'));
const AgentSetsPage = lazy(() => import('./pages/app/agentSets'));
const CapabilitiesPage = lazy(() => import('./pages/app/capabilities'));
const WorkstationsPage = lazy(() => import('./pages/app/workstations'));
const SessionsPage = lazy(() => import('./pages/app/sessions'));
const PersonnelPage = lazy(() => import('./pages/app/personnel'));
const FilesPage = lazy(() => import('./pages/app/files'));
const ChatPage = lazy(() => import('./pages/app/chat'));

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
          <Route path="workstations" element={<WorkstationsPage />} />
          <Route path="sessions" element={<SessionsPage />} />
          <Route path="personnel" element={<PersonnelPage />} />
          <Route path="files" element={<FilesPage />} />
          <Route path="chat" element={<ChatPage />} />
          <Route path="chat/:id" element={<ChatPage />} />
          <Route path="*" element={<Navigate to="/projects" replace />} />
        </Route>
        <Route path="*" element={<NotFoundPage />} />
      </Routes>
    </BrowserRouter>,
);