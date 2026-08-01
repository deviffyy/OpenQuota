import { en } from './locales/en';
import { zhCN } from './locales/zh-CN';
import { zhTW } from './locales/zh-TW';

export type UiLanguage = 'en' | 'zh-CN' | 'zh-TW';

const messages = { en, 'zh-CN': zhCN, 'zh-TW': zhTW };
const systemLanguage = typeof navigator === 'undefined' ? 'en' : navigator.language;
let currentLanguage: UiLanguage = systemLanguage.startsWith('zh-TW') || systemLanguage.startsWith('zh-Hant')
  ? 'zh-TW'
  : systemLanguage.startsWith('zh') ? 'zh-CN' : 'en';
let hasExplicitLanguage = false;

export function setUiLanguage(language: UiLanguage) { currentLanguage = language; hasExplicitLanguage = true; }
export function getUiLanguage() { return currentLanguage; }
export function getFormatLocale() { return hasExplicitLanguage ? currentLanguage : undefined; }
export function t(key: keyof typeof en) { return messages[currentLanguage][key]; }
export function localeFor(language = currentLanguage) { return language; }
