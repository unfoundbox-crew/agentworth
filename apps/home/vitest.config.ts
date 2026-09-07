import path from 'path';
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

// Separate from vite.config.ts (which apps/home's tsconfig doesn't type-check)
// so `npm run test` doesn't need the dev server's proxy or /home base path.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@ui': path.resolve(__dirname, '../../packages/ui'),
    },
  },
  test: {
    environment: 'node',
  },
});
