import { describe, expect, it } from 'vitest';
import layoutCss from '../styles/layout.css?raw';
import sharedComponentCss from '../styles/components.css?raw';
import tokensCss from '../styles/tokens.css?raw';
import customizeDetail from './CustomizeProviderDetail.svelte?raw';
import customizeList from './CustomizeProviderList.svelte?raw';
import dashboard from './Dashboard.svelte?raw';
import settings from './SettingsScreen.svelte?raw';
import { coLocatedComponentCss, componentSources } from './uiStyleSources';
import { en } from './locales/en';
import { zhCN } from './locales/zh-CN';
import { zhTW } from './locales/zh-TW';
import {
  getLanguagePreference,
  getUiLanguage,
  messages,
  normalizeLanguagePreference,
  resolveSystemLanguage,
  setUiLanguage,
  t,
} from './i18n';
import app from '../App.svelte?raw';

const css = `${tokensCss}\n${layoutCss}\n${sharedComponentCss}\n${coLocatedComponentCss}`;

describe('native UI language contract', () => {
  it('uses the platform system font and reference type sizes', () => {
    expect(css).toMatch(/font-family:\s*system-ui,/);
    expect(css).not.toMatch(/font-family:\s*Inter/);
    expect(css).toMatch(/\.provider-header h1\s*{[^}]*font-size: 14px;[^}]*font-weight: 600;/s);
    expect(css).toMatch(/\.provider-list-main b\s*{[^}]*font-size: 14px;[^}]*font-weight: 600;/s);
    expect(css).toMatch(/\.setting-row\s*{[^}]*font-size: 13px;/s);
  });

  it('keeps the critical flame colored while its warning copy stays secondary', () => {
    expect(css).not.toMatch(/\.metric__heading span\s*{/);
    expect(css).toMatch(
      /\.metric__heading \.pace-warning__icon\s*{[^}]*color: var\(--meter-critical\);/s,
    );
    expect(css).toMatch(/\.metric__heading \.pace-warning\s*{[^}]*color: var\(--secondary\);/s);
  });

  it('keeps spend providers visually distinct in both appearances', () => {
    for (const provider of ['claude', 'codex', 'cursor', 'grok', 'opencode', 'openrouter']) {
      expect(tokensCss).toContain(`--provider-${provider}:`);
    }
    expect(tokensCss).toMatch(
      /@media \(prefers-color-scheme: dark\)[\s\S]*--provider-cursor: #f5f5f7;[\s\S]*--provider-opencode: #aeaeb2;/,
    );
    expect(tokensCss).toMatch(
      /:root\[data-theme='dark'\][\s\S]*--provider-cursor: #f5f5f7;[\s\S]*--provider-opencode: #aeaeb2;/,
    );
  });

  it('keeps Customize concise and free of duplicate status and count copy', () => {
    expect(customizeList).toContain("t('notifications')}, {t('appearance')}");
    expect(customizeList).toContain("t('metricCount'");
    expect(customizeList).not.toContain('Detected locally');
    expect(customizeList).not.toContain('screen-intro');
    expect(customizeList).not.toContain('pinned\n');
    expect(customizeDetail).toContain("t('dragMetricsHere')");
    expect(customizeDetail).toContain("t('starredForMenuBar')");
    expect(customizeDetail).toContain("t('removedFromMenuBar')");
    expect(customizeDetail).toContain("t('pinnedLimit')");
    expect(customizeDetail).not.toContain('provider-toggle-row');
    expect(customizeDetail).not.toContain('section-divider');
    expect(customizeDetail).not.toContain('of 2 pinned');
  });

  it('uses the shared Settings labels and single-line control rows', () => {
    for (const key of [
      'general',
      'showTotalSpend',
      'launchAtLogin',
      'globalShortcut',
      'iconStyle',
      'appearance',
      'usageDisplay',
      'notifications',
      'advanced',
      'updates',
      'autoUpdates',
      'checkUpdates',
    ]) {
      expect(settings).toContain(`t('${key}')`);
    }
    expect(settings).toContain("{ value: 'system', label: t('auto') }");
    expect(settings).toContain("{ value: 'twelveHour', label: t('twelveHour') }");
    expect(settings).toContain("{ value: 'twentyFourHour', label: t('twentyFourHour') }");
    expect(settings).not.toContain('<h2>Startup</h2>');
    expect(settings).not.toContain('Automatic Checks');
    expect(settings).not.toContain('Combined cost and token summary.');
    expect(settings).not.toContain('Show projections even when usage is healthy.');
    expect(settings).not.toContain('>×</button');
  });

  it('keeps dashboard onboarding, empty state, and menus on the shared wording', () => {
    expect(dashboard).toContain("t('welcome')");
    expect(dashboard).toContain("t('openCustomize')");
    expect(dashboard).toContain("t('customizeHint')");
    expect(dashboard).toContain("t('customizeMenu')");
    expect(dashboard).toContain("t('refreshProvider'");
    expect(dashboard).not.toContain('Providers Detected');
    expect(dashboard).not.toContain('Starter Provider');
    expect(dashboard).not.toContain("Expand'} On Demand");
    expect(dashboard).not.toContain('>×</button');
  });

  it('resolves system languages and falls back to English', () => {
    expect(resolveSystemLanguage('zh-CN')).toBe('zh-CN');
    expect(resolveSystemLanguage('zh-TW')).toBe('zh-TW');
    expect(resolveSystemLanguage('zh-HK')).toBe('zh-TW');
    expect(resolveSystemLanguage('zh-MO')).toBe('zh-TW');
    expect(resolveSystemLanguage('zh-SG')).toBe('zh-CN');
    expect(resolveSystemLanguage('fr-FR')).toBe('en');
  });

  it('updates translations when the persisted preference changes', () => {
    setUiLanguage('en');
    expect(t('settings')).toBe('Settings');
    setUiLanguage('zh-CN');
    expect(getUiLanguage()).toBe('zh-CN');
    expect(t('settings')).toBe('设置');
    setUiLanguage('system');
  });

  it('keeps the system preference while resolving a display language and safely falls back', () => {
    setUiLanguage('system');
    expect(getLanguagePreference()).toBe('system');
    setUiLanguage('invalid-locale');
    expect(getLanguagePreference()).toBe('en');
    expect(getUiLanguage()).toBe('en');
    expect(t('settings')).toBe('Settings');
    expect(normalizeLanguagePreference(undefined)).toBe('system');
    expect(normalizeLanguagePreference('zh-TW')).toBe('zh-TW');
    expect(normalizeLanguagePreference('broken')).toBe('en');
  });

  it('keeps every language resource aligned with the English keys', () => {
    expect(Object.keys(zhCN).sort()).toEqual(Object.keys(en).sort());
    expect(Object.keys(zhTW).sort()).toEqual(Object.keys(en).sort());
    expect(Object.keys(messages['zh-CN']).sort()).toEqual(Object.keys(en).sort());
    expect(Object.keys(messages['zh-TW']).sort()).toEqual(Object.keys(en).sort());
  });

  it('keeps Traditional Chinese independent from Simplified Chinese resources', () => {
    expect(zhTW.settings).toBe('設定');
    expect(zhTW.globalShortcut).toBe('全域快速鍵');
    expect(zhTW.copyLogPath).toBe('複製記錄路徑');
    expect(zhTW).not.toBe(zhCN);
  });

  it('does not style the shortcut button through localized accessible text', () => {
    expect(settings).toContain('clear-shortcut-button');
    expect(settings).not.toContain("button[aria-label='Clear global shortcut']");
    expect(css).not.toMatch(/\[(?:aria-label|title|data-tooltip)=/);
  });

  it('does not use translated strings as logic identifiers', () => {
    const source = componentSources.join('\n');
    expect(source).not.toMatch(/(?:===|!==)\s*t\(/);
    expect(source).not.toMatch(/querySelector[^\n]*(?:aria-label|title|data-tooltip)/);
    expect(source).not.toMatch(/\.replace\(\s*\/\^Resets/);
  });

  it('updates language without re-mounting the application', () => {
    expect(app).not.toMatch(/\{#key\s+settingsState\?\.settings\.language/);
    expect(app).toContain('data-language={$uiLanguage}');
  });
});
