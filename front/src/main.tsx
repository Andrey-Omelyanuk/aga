import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { store } from './store';
import { StoreContext } from './store-hooks';
import './styles/globals.css';

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>
  </React.StrictMode>,
);