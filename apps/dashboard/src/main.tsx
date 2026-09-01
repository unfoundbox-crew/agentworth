import React from 'react';
import ReactDOM from 'react-dom/client';
import { ExplorerShell } from './shell/ExplorerShell';
import './index.css';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ExplorerShell />
  </React.StrictMode>
);
