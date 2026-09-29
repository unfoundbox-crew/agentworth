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
  // `dist` is Vite's default outDir, relative to this config's directory, and it
  // is named explicitly here so it cannot be inherited or overridden from
  // elsewhere: apps/dashboard/dist once ended up holding apps/home's build, and
  // because both apps emit an index.html the mix-up was invisible -- `/` served
  // home's shell while looking like a broken dashboard. See the emptyOutDir note
  // below and the discriminating assertions in .github/workflows/ci.yml.
  build: {
    outDir: 'dist',
    // Wipe the directory on every build, so a stale or foreign index.html/assets
    // bundle can never survive into the rust-embed folder alongside this app's.
    emptyOutDir: true,
  },
  resolve: {
    alias: {
      '@ui': path.resolve(__dirname, '../../packages/ui'),
      // Single copy of the Rust-generated API contract fixtures; mirrors the
      // tsconfig.json "@api-fixtures/*" path.
      '@api-fixtures': path.resolve(__dirname, '../cli/tests/fixtures/api-contract'),
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
