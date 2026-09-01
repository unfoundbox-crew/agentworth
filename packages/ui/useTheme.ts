import { useState, useEffect } from 'react';

export type Theme = 'system' | 'light' | 'dark';

export function getStoredTheme(): Theme {
  if (typeof window !== 'undefined') {
    const stored = localStorage.getItem('agentworth_theme') as Theme | null;
    if (stored === 'light' || stored === 'dark' || stored === 'system') {
      return stored;
    }
  }
  return 'light';
}

export function applyThemeToDocument(theme: Theme) {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  const isDark =
    theme === 'dark' ||
    (theme === 'system' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches);

  // .dark drives Tailwind's class-based dark: utilities used across the app.
  if (isDark) {
    root.classList.add('dark');
  } else {
    root.classList.remove('dark');
  }

  // data-theme drives the --mv-* token cascade (design.md's three-state
  // contract): explicit light/dark force the override, "system" removes
  // the attribute so the guarded prefers-color-scheme block decides.
  if (theme === 'light' || theme === 'dark') {
    root.setAttribute('data-theme', theme);
  } else {
    root.removeAttribute('data-theme');
  }
}

export function useTheme() {
  const [theme, setThemeState] = useState<Theme>(getStoredTheme);

  const setTheme = (newTheme: Theme) => {
    setThemeState(newTheme);
    if (typeof window !== 'undefined') {
      localStorage.setItem('agentworth_theme', newTheme);
      applyThemeToDocument(newTheme);
      window.dispatchEvent(new CustomEvent('agentworth-theme-change', { detail: newTheme }));
    }
  };

  useEffect(() => {
    applyThemeToDocument(theme);

    const handleThemeChange = (e: Event) => {
      const customEvent = e as CustomEvent<Theme>;
      if (customEvent.detail && customEvent.detail !== theme) {
        setThemeState(customEvent.detail);
      }
    };

    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    const handleMediaChange = () => {
      const current = getStoredTheme();
      if (current === 'system') {
        applyThemeToDocument('system');
      }
    };

    window.addEventListener('agentworth-theme-change', handleThemeChange);
    mediaQuery.addEventListener('change', handleMediaChange);

    return () => {
      window.removeEventListener('agentworth-theme-change', handleThemeChange);
      mediaQuery.removeEventListener('change', handleMediaChange);
    };
  }, [theme]);

  return { theme, setTheme };
}

