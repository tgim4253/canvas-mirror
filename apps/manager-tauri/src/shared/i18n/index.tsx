import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from 'react';
import {
  hasTranslationKey,
  normalizeLocale,
  toDocumentLang,
  translateForLocale,
  type Locale,
  type TranslationValues,
} from './resources';

const LOCALE_STORAGE_KEY = 'app.locale';

type I18nContextValue = {
  locale: Locale;
  setLocale: (nextLocale: string) => void;
  t: (key: string, values?: TranslationValues) => string;
  translateMaybe: (message: string, values?: TranslationValues) => string;
};

const I18nContext = createContext<I18nContextValue | null>(null);

function readSearchLocale() {
  if (typeof window === 'undefined') {
    return null;
  }

  const params = new URLSearchParams(window.location.search);
  return params.get('lang') ?? params.get('locale');
}

function readStoredLocale() {
  if (typeof window === 'undefined') {
    return null;
  }

  try {
    return window.localStorage.getItem(LOCALE_STORAGE_KEY);
  } catch {
    return null;
  }
}

export function detectPreferredLocale(): Locale {
  const searchLocale = readSearchLocale();
  if (searchLocale) {
    return normalizeLocale(searchLocale);
  }

  const storedLocale = readStoredLocale();
  if (storedLocale) {
    return normalizeLocale(storedLocale);
  }

  if (typeof navigator !== 'undefined') {
    for (const candidate of navigator.languages) {
      const locale = normalizeLocale(candidate);
      if (locale !== 'en' || candidate.trim().toLowerCase().startsWith('en')) {
        return locale;
      }
    }

    if (navigator.language) {
      return normalizeLocale(navigator.language);
    }
  }

  return 'en';
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() => detectPreferredLocale());

  useEffect(() => {
    document.documentElement.lang = toDocumentLang(locale);

    try {
      window.localStorage.setItem(LOCALE_STORAGE_KEY, locale);
    } catch {
      // Ignore storage failures and keep the in-memory locale.
    }
  }, [locale]);

  const t = (key: string, values?: TranslationValues) => translateForLocale(locale, key, values);
  const translateMaybe = (message: string, values?: TranslationValues) =>
    hasTranslationKey(message) ? t(message, values) : message;

  return (
    <I18nContext.Provider
      value={{
        locale,
        setLocale: nextLocale => setLocaleState(normalizeLocale(nextLocale)),
        t,
        translateMaybe,
      }}
    >
      {children}
    </I18nContext.Provider>
  );
}

export function useI18n() {
  const context = useContext(I18nContext);
  if (!context) {
    throw new Error('I18nProvider is missing');
  }

  return context;
}

export type { Locale, TranslationValues } from './resources';
