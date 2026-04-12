import enCommon from '../../locales/en/common.json';
import jpCommon from '../../locales/jp/common.json';
import koCommon from '../../locales/ko/common.json';

export const resources = {
  en: enCommon,
  ko: koCommon,
  jp: jpCommon,
} as const;

export type Locale = keyof typeof resources;
export type TranslationKey = keyof typeof resources.en;
export type TranslationValues = Record<string, string | number>;

const LOCALE_ALIASES: Record<string, Locale> = {
  en: 'en',
  'en-us': 'en',
  'en-gb': 'en',
  ko: 'ko',
  'ko-kr': 'ko',
  ja: 'jp',
  jp: 'jp',
  'ja-jp': 'jp',
};

export function normalizeLocale(rawLocale?: string | null): Locale {
  if (!rawLocale) {
    return 'en';
  }

  const normalizedLocale = rawLocale.trim().toLowerCase().replaceAll('_', '-');
  if (normalizedLocale in LOCALE_ALIASES) {
    return LOCALE_ALIASES[normalizedLocale];
  }

  const languageCode = normalizedLocale.split('-', 1)[0];
  if (languageCode && languageCode in LOCALE_ALIASES) {
    return LOCALE_ALIASES[languageCode];
  }

  return 'en';
}

export function toDocumentLang(locale: Locale) {
  return locale === 'jp' ? 'ja' : locale;
}

export function hasTranslationKey(key: string): key is TranslationKey {
  return key in resources.en;
}

export function interpolate(
  template: string,
  values?: TranslationValues,
) {
  if (!values) {
    return template;
  }

  return template.replace(/\{\{\s*(\w+)\s*\}\}/g, (match, token) => {
    const replacement = values[token];
    return replacement === undefined ? match : String(replacement);
  });
}

export function translateForLocale(
  locale: Locale,
  key: string,
  values?: TranslationValues,
) {
  const localizedTemplate = hasTranslationKey(key)
    ? (resources[locale][key] ?? resources.en[key])
    : undefined;

  return interpolate(localizedTemplate ?? key, values);
}
