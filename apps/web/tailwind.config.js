/** @type {import('tailwindcss').Config} */
export default {
  darkMode: 'class',
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      fontFamily: {
        // MotionVector design system: Geist / Geist Mono, loaded in index.html
        // and exposed as CSS vars in index.css. See /design.md §1.
        mono: [
          'var(--font-mono)',
          'Geist Mono',
          'ui-monospace',
          'SFMono-Regular',
          'Menlo',
          'Monaco',
          'Consolas',
          '"Liberation Mono"',
          '"Courier New"',
          'monospace',
        ],
        sans: [
          'var(--font-sans)',
          'Geist',
          '-apple-system',
          'BlinkMacSystemFont',
          '"Segoe UI"',
          'Helvetica',
          'Arial',
          'sans-serif',
        ],
      },
      colors: {
        bg: {
          light: '#fdfdfd',
          subtle: '#f8f9fa',
          muted: '#f1f3f5',
          border: '#e5e7eb',
          darkBorder: '#111827',
        },
        brand: {
          black: '#000000',
          nearBlack: '#0a0a0c',
          charcoal: '#18181b',
          gray: '#52525b',
          lightGray: '#a1a1aa',
          border: '#e4e4e7',
          success: '#15803d',
          failure: '#b91c1c',
          warn: '#b45309',
        },
        // MotionVector --mv-* tokens (see /design.md §2). Theme-aware by
        // construction — components using these need no dark: variant.
        ground: 'var(--mv-ground)',
        surface: {
          DEFAULT: 'var(--mv-surface)',
          2: 'var(--mv-surface-2)',
          3: 'var(--mv-surface-3)',
        },
        border: {
          DEFAULT: 'var(--mv-border)',
          soft: 'var(--mv-border-soft)',
        },
        ink: 'var(--mv-ink)',
        text: 'var(--mv-text)',
        muted: 'var(--mv-muted)',
        faint: 'var(--mv-faint)',
        accent: {
          DEFAULT: 'var(--mv-accent)',
          soft: 'var(--mv-accent-soft)',
          border: 'var(--mv-accent-border)',
          contrast: 'var(--mv-accent-contrast)',
        },
        // Semantic status — kept separate from --mv-accent (design.md hard
        // rule). Verified/warn/unverified states across the dashboard.
        success: {
          DEFAULT: 'var(--mv-success)',
          soft: 'var(--mv-success-soft)',
          border: 'var(--mv-success-border)',
        },
        warn: {
          DEFAULT: 'var(--mv-warn)',
          soft: 'var(--mv-warn-soft)',
          border: 'var(--mv-warn-border)',
        },
        danger: {
          DEFAULT: 'var(--mv-danger)',
          soft: 'var(--mv-danger-soft)',
          border: 'var(--mv-danger-border)',
        },
      },
      boxShadow: {
        'brutal': '2px 2px 0px 0px #000000',
        'brutal-lg': '4px 4px 0px 0px #000000',
        'brutal-sm': '1px 1px 0px 0px #000000',
      },
    },
  },
  plugins: [],
}
