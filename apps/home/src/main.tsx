import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { gateway } from './ws/client';
import './index.css';

gateway.connect();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
