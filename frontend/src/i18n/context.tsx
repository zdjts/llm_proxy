import { createContext, useContext, useState, useCallback, type ReactNode } from 'react';
import type { Locale, Messages } from './types';
import en from './locales/en';
import zhCN from './locales/zh-CN';

const locales: Record<Locale, Messages> = { en, 'zh-CN': zhCN };

function detectLocale(): Locale {
  if (typeof localStorage !== 'undefined') {
    const saved = localStorage.getItem('dashboard-locale');
    if (saved === 'en' || saved === 'zh-CN') return saved;
  }
  if (typeof navigator !== 'undefined') {
    const lang = navigator.language;
    if (lang.startsWith('zh')) return 'zh-CN';
  }
  return 'en';
}

interface LocaleContextValue {
  locale: Locale;
  t: Messages;
  setLocale: (l: Locale) => void;
}

const LocaleContext = createContext<LocaleContextValue | null>(null);

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(detectLocale);

  const setLocale = useCallback((l: Locale) => {
    setLocaleState(l);
    localStorage.setItem('dashboard-locale', l);
  }, []);

  return (
    <LocaleContext.Provider value={{ locale, t: locales[locale], setLocale }}>
      {children}
    </LocaleContext.Provider>
  );
}

export function useLocale() {
  const ctx = useContext(LocaleContext);
  if (!ctx) throw new Error('useLocale must be used within LocaleProvider');
  return ctx;
}
