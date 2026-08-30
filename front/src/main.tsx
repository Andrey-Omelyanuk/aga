import React from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom';
import { App } from './App';
import { store } from './store';
import { StoreContext } from './store-hooks';
import { Projects } from './pages/Projects';
import { Workstations } from './pages/Workstations';
import { Sessions } from './pages/Sessions';
import { Personnel } from './pages/Personnel';
import { Files } from './pages/Files';
import { Chat } from './pages/Chat';
import './styles/globals.css';

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <StoreContext.Provider value={store}>
      <BrowserRouter>
        <Routes>
          <Route element={<App />}>
            <Route index element={<Navigate to="/projects" replace />} />
            <Route path="projects" element={<Projects />} />
            <Route path="workstations" element={<Workstations />} />
            <Route path="sessions" element={<Sessions />} />
            <Route path="personnel" element={<Personnel />} />
            <Route path="files" element={<Files />} />
            <Route path="chat" element={<Chat />} />
            <Route path="chat/:id" element={<Chat />} />
            <Route path="*" element={<Navigate to="/projects" replace />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </StoreContext.Provider>
  </React.StrictMode>,
);