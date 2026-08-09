import { en } from './locales/en';
import { zhCN } from './locales/zh-CN';
import { zhTW } from './locales/zh-TW';
import { fromStore, writable } from 'svelte/store';

export type UiLanguage = 'en' | 'zh-CN' | 'zh-TW';
export type LanguagePreference = 'system' | UiLanguage;

export type TranslationKey = keyof typeof en;
export type PartialCatalog = Partial<Record<TranslationKey, string>>;

const catalogs: Record<UiLanguage, PartialCatalog> = { en, 'zh-CN': zhCN, 'zh-TW': zhTW };
export const messages = {
  en,
  'zh-CN': { ...en, ...zhCN },
  'zh-TW': { ...en, ...zhTW },
};

export function normalizeLanguagePreference(value: unknown): LanguagePreference {
  if (value === undefined || value === null) return 'system';
  if (value === 'system' || value === 'en' || value === 'zh-CN' || value === 'zh-TW') return value;
  return 'en';
}

function normalizeUiLanguage(value: unknown): UiLanguage | undefined {
  return value === 'en' || value === 'zh-CN' || value === 'zh-TW' ? value : undefined;
}

export function normalizeLocaleTag(language: string): string {
  return language.trim().split(/[.@]/, 1)[0].replace(/_/g, '-').toLowerCase();
}

export function resolveSystemLanguage(
  language = typeof navigator === 'undefined' ? '' : navigator.language,
): UiLanguage {
  const normalized = normalizeLocaleTag(language);
  if (
    normalized === 'zh-tw' ||
    normalized === 'zh-hk' ||
    normalized === 'zh-mo' ||
    normalized.startsWith('zh-hant')
  )
    return 'zh-TW';
  if (normalized.startsWith('zh')) return 'zh-CN';
  return 'en';
}

let preference: LanguagePreference = 'system';
let currentLanguage: UiLanguage = resolveSystemLanguage();
export const uiLanguage = writable<UiLanguage>(currentLanguage);
const reactiveUiLanguage = fromStore(uiLanguage);

export function setUiLanguage(value: unknown, resolvedLanguage?: unknown) {
  preference = normalizeLanguagePreference(value);
  currentLanguage =
    preference === 'system'
      ? (normalizeUiLanguage(resolvedLanguage) ?? resolveSystemLanguage())
      : preference;
  uiLanguage.set(currentLanguage);
}
export function getUiLanguage() {
  return reactiveUiLanguage.current;
}
export function getLanguagePreference() {
  return preference;
}
export function getFormatLocale() {
  return currentLanguage;
}
export function translateFromCatalog(
  catalog: PartialCatalog,
  key: TranslationKey,
  values: Record<string, string | number> = {},
) {
  const template = catalog[key] ?? en[key] ?? String(key);
  return template.replace(/\{(\w+)\}/g, (_, name) => String(values[name] ?? `{${name}}`));
}
export function t(key: TranslationKey, values: Record<string, string | number> = {}) {
  return translateFromCatalog(catalogs[getUiLanguage()] ?? {}, key, values);
}
export function localeFor(language = getUiLanguage()) {
  return language;
}
