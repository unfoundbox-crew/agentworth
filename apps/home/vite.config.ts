import path from 'path';
import { defineConfig } from 'vite';

// HOME_GATEWAY picks the WebSocket target: the mock on 7777 by default, or a real
// `archie serve --home` port for a preview against the live fleet.
const gateway = process.env.HOME_GATEWAY ?? '127.0.0.1:7777';
const api = process.env.HOME_API ?? '127.0.0.1:3000';
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
      '/ws': { target: `ws://${gateway}`, ws: true },
      '/api': { target: `http://${api}`, changeOrigin: true },
    },
  },
});
