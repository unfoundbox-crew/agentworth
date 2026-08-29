/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      fontFamily: {
        mono: [
          'JetBrains Mono',
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
          'Inter',
          '-apple-system',
          'BlinkMacSystemFont',
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
