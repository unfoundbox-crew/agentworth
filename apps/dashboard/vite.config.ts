import path from 'path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// apps/dashboard is the local app served by `agentworth serve` — it talks
// to the Rust server's /api/* over the same origin in production, and
// through this dev proxy when run with `npm run dev`.
export default defineConfig({
  base: './',
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
