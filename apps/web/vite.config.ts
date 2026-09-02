import path from 'path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// apps/web is the marketing site only — no /api proxy. The dashboard
// (apps/dashboard) is the app that talks to the Rust server's /api/*.
export default defineConfig({
  // Absolute, not './'. Pre-rendered routes live in their own directories
  // (/blog/<slug>/index.html), where a relative asset path resolves against
  // the route directory and 404s.
  base: '/',
  plugins: [react()],
  resolve: {
    alias: {
      '@ui': path.resolve(__dirname, '../../packages/ui'),
    },
  },
  server: {
    port: 5173,
  },
});
