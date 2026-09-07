/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      fontFamily: {
        mono: ['var(--font-mono)', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
      },
      colors: {
        ground: 'var(--ground)',
        panel: 'var(--panel)',
        line: 'var(--line)',
        ink: 'var(--ink)',
        muted: 'var(--muted)',
        dim: 'var(--dim)',
      },
    },
  },
  plugins: [],
};
