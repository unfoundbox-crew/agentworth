import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { ExplorerShell } from './shell/ExplorerShell';
import './index.css';

// /explorer and /s/<id> render the keyboard-first shell; everything else
// (including /) renders the existing app untouched.
const pathname = window.location.pathname;
const isExplorerRoute = pathname.startsWith('/explorer') || pathname.startsWith('/s/');

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    {isExplorerRoute ? <ExplorerShell /> : <App />}
  </React.StrictMode>
);
