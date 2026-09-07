import path from 'path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// apps/home is served by `archie serve` at /home in production, from the same
// binary as apps/dashboard. In dev, /ws goes to whatever is on 7777: the Rust
// gateway once it exists, `npm run mock` until then.
export default defineConfig({
  base: '/home/',
  plugins: [react()],
  resolve: {
    alias: {
      '@ui': path.resolve(__dirname, '../../packages/ui'),
    },
  },
  server: {
    port: 5175,
    proxy: {
      '/ws': { target: 'ws://127.0.0.1:7777', ws: true },
      '/api': { target: 'http://127.0.0.1:3000', changeOrigin: true },
    },
  },
});
