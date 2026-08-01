import { en } from './locales/en';
import { zhCN } from './locales/zh-CN';
import { zhTW } from './locales/zh-TW';
import { fromStore, writable } from 'svelte/store';

export type UiLanguage = 'en' | 'zh-CN' | 'zh-TW';
export type LanguagePreference = 'system' | UiLanguage;

export const messages = { en, 'zh-CN': { ...en, ...zhCN }, 'zh-TW': { ...en, ...zhTW } };

export function normalizeLanguagePreference(value: unknown): LanguagePreference {
  if (value === undefined || value === null) return 'system';
  if (value === 'system' || value === 'en' || value === 'zh-CN' || value === 'zh-TW') return value;
  return 'en';
}

export function resolveSystemLanguage(
  language = typeof navigator === 'undefined' ? '' : navigator.language,
): UiLanguage {
  const normalized = language.toLowerCase();
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

export function setUiLanguage(value: unknown) {
  preference = normalizeLanguagePreference(value);
  currentLanguage = preference === 'system' ? resolveSystemLanguage() : preference;
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
export function t(key: keyof typeof en, values: Record<string, string | number> = {}) {
  const dictionary = messages[getUiLanguage()] ?? en;
  const template = dictionary[key] ?? en[key] ?? String(key);
  return template.replace(/\{(\w+)\}/g, (_, name) => String(values[name] ?? `{${name}}`));
}
export function localeFor(language = getUiLanguage()) {
  return language;
}
