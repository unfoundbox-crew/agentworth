/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      fontFamily: {
        mono: ['var(--font-mono)', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
      },
      colors: {
        ground: 'var(--mv-ground)',
        panel: 'var(--mv-surface)',
        line: 'var(--mv-border)',
        ink: 'var(--mv-ink)',
        text: 'var(--mv-text)',
        muted: 'var(--mv-muted)',
        dim: 'var(--mv-faint)',
        accent: 'var(--mv-accent)',
        warn: 'var(--mv-warn)',
        success: 'var(--mv-success)',
        danger: 'var(--mv-danger)',
      },
    },
  },
  plugins: [],
};
