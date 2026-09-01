import React from 'react';
import { Theme, useTheme } from '../hooks/useTheme';

/**
 * System / Light / Dark control — design.md "The toggle". Persists to
 * localStorage via useTheme (key: agentworth_theme); light is the default
 * for a first-time visitor.
 */
export const ThemeToggle: React.FC<{ className?: string }> = ({ className = '' }) => {
  const { theme, setTheme } = useTheme();

  const options: { value: Theme; label: string }[] = [
    { value: 'system', label: 'System' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
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
          {opt.label}
        </button>
      ))}
    </div>
  );
};
