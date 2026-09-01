import React from 'react';
import { Theme, useTheme } from './useTheme';

/**
 * System / Light / Dark control — design.md "The toggle". Persists to
 * localStorage via useTheme (key: agentworth_theme); light is the default
 * for a first-time visitor.
 */
export const ThemeToggle: React.FC<{ className?: string }> = ({ className = '' }) => {
  const { theme, setTheme } = useTheme();

  // Drawn here rather than pulled from a library: motionvector and spacepilot
  // ship brand marks and favicons but no icon set, and adding a dependency for
  // three glyphs is not worth it. 20x20 grid, 1.5 stroke, currentColor.
  const options: { value: Theme; label: string; icon: React.ReactNode }[] = [
    {
      value: 'system',
      label: 'System',
      icon: (
        <>
          <rect x="2.5" y="4" width="15" height="10" rx="1.5" />
          <path d="M7 17h6M10 14v3" />
        </>
      ),
    },
    {
      value: 'light',
      label: 'Light',
      icon: (
        <>
          <circle cx="10" cy="10" r="3.5" />
          <path d="M10 2v2M10 16v2M2 10h2M16 10h2M4.6 4.6l1.4 1.4M14 14l1.4 1.4M15.4 4.6L14 6M6 14l-1.4 1.4" />
        </>
      ),
    },
    {
      value: 'dark',
      label: 'Dark',
      icon: <path d="M16 11.2A6.5 6.5 0 1 1 8.8 4a5.2 5.2 0 0 0 7.2 7.2z" />,
    },
  ];

  return (
    <div className={`theme-toggle ${className}`} role="group" aria-label="Theme">
      {options.map((opt) => (
        <button
          key={opt.value}
          type="button"
          data-theme-choice={opt.value}
          aria-pressed={theme === opt.value}
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            setTheme(opt.value);
          }}
          title={`Set theme to ${opt.label}`}
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 20 20"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            {opt.icon}
          </svg>
          <span className="sr-only">{opt.label}</span>
        </button>
      ))}
    </div>
  );
};
