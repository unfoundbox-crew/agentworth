import React from 'react';
import { Sun, Moon, Monitor } from 'lucide-react';
import { Theme, useTheme } from '../hooks/useTheme';

export const ThemeToggle: React.FC<{ className?: string }> = ({ className = '' }) => {
  const { theme, setTheme } = useTheme();

  const options: { value: Theme; label: string; icon: React.ReactNode }[] = [
    { value: 'light', label: 'Light', icon: <Sun className="w-3.5 h-3.5" /> },
    { value: 'dark', label: 'Dark', icon: <Moon className="w-3.5 h-3.5" /> },
    { value: 'system', label: 'System', icon: <Monitor className="w-3.5 h-3.5" /> },
  ];

  return (
    <div className={`inline-flex items-center rounded-md border border-neutral-200 dark:border-neutral-800 bg-neutral-100/80 dark:bg-neutral-900 p-0.5 font-mono text-xs ${className}`}>
      {options.map((opt) => (
        <button
          key={opt.value}
          type="button"
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            setTheme(opt.value);
          }}
          title={`Set theme to ${opt.label}`}
          className={`p-1.5 rounded transition-colors ${
            theme === opt.value
              ? 'bg-white dark:bg-neutral-800 text-neutral-950 dark:text-white shadow-xs font-semibold'
              : 'text-neutral-500 hover:text-neutral-900 dark:text-neutral-400 dark:hover:text-neutral-200'
          }`}
          aria-label={`Switch to ${opt.label} theme`}
        >
          {opt.icon}
        </button>
      ))}
    </div>
  );
};
