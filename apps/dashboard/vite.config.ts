import path from 'path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// apps/dashboard is the local app served by `agentworth serve` — it talks
// to the Rust server's /api/* over the same origin in production, and
// through this dev proxy when run with `npm run dev`.
export default defineConfig({
  // MUST be absolute. With './' the emitted src is "./assets/...", which the
  // browser resolves against the current route — so on /s/<id> it requests
  // /s/assets/... , the SPA fallback answers with index.html, and the module
  // script dies on a MIME check before React ever mounts. Every deep link was
  // a blank page; only / worked, which is why it looked fine.
  base: '/',
  plugins: [react()],
  resolve: {
    alias: {
      '@ui': path.resolve(__dirname, '../../packages/ui'),
    },
  },
  server: {
    port: 5174,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:3000',
        changeOrigin: true,
      },
    },
  },
});
