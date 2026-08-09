import { afterEach, describe, expect, it } from 'vitest';
import { codexState, providerCatalog } from '../test/appFixtures';
import { setUiLanguage } from './i18n';
import {
  localizedNoticeMessage,
  localizedNoticeTitle,
  localizedStatusText,
  ProviderCatalogIndex,
  usageSourceNote,
} from './metrics';
import type { ProviderNotice, StatusMetric } from './types';

afterEach(() => setUiLanguage('en'));

describe('provider catalog index', () => {
  it('indexes provider identity and metric metadata from bootstrap data', () => {
    const catalog = new ProviderCatalogIndex(providerCatalog);

    expect(catalog.displayName('codex')).toBe('Codex');
    expect(catalog.displayName('codex', { codex: '  Work Account  ' })).toBe('Work Account');
    expect(catalog.metric('claude.session')).toMatchObject({
      label: 'Session',
      source: { kind: 'quota', sourceId: 'session', sessionWindow: true },
    });
    expect(catalog.supportsSpend('claude')).toBe(true);
    expect(catalog.supportsSpend('antigravity')).toBe(false);
    expect(catalog.metric('openrouter.balance')).toMatchObject({
      label: 'Balance',
      source: { kind: 'value', sourceId: 'balance' },
    });
    expect(catalog.localUsageSourceNote('codex')).toBe('From your Codex logs (estimated)');
    expect(catalog.provider('codex')?.links).toEqual([
      { label: 'Status', url: 'https://status.openai.com/' },
      { label: 'Dashboard', url: 'https://chatgpt.com/codex/settings/usage' },
    ]);
  });

  it('uses safe unknown-provider fallbacks without borrowing another provider identity', () => {
    const catalog = new ProviderCatalogIndex(providerCatalog);

    expect(catalog.displayName('future-provider')).toBe('future-provider');
    expect(catalog.metric('future-provider.session')).toBeUndefined();
    expect(catalog.localUsageSourceNote('future-provider')).toBe(
      'From your future-provider usage history',
    );
  });

  it('prefers the snapshot usage source when an additional local source contributed', () => {
    const catalog = new ProviderCatalogIndex(providerCatalog);
    const snapshot = structuredClone(codexState.snapshot!);
    snapshot.usage.last30Days!.modelBreakdown = {
      models: [],
      sourceNote: 'From your Codex logs and pi (estimated)',
      sourceKey: 'estimatedLogsWithPiSource',
    };

    expect(usageSourceNote(catalog, snapshot)).toBe('From your Codex logs and pi (estimated)');
    snapshot.usage.last30Days!.modelBreakdown!.sourceKey = null;
    expect(usageSourceNote(catalog, snapshot)).toBe('From your Codex logs (estimated)');
    snapshot.usage.last30Days!.modelBreakdown = null;
    snapshot.usage.today!.modelBreakdown = null;
    snapshot.usage.yesterday!.modelBreakdown = null;
    expect(usageSourceNote(catalog, snapshot)).toBe('From your Codex logs (estimated)');
  });

  it('rejects duplicate provider and metric ids at the frontend boundary', () => {
    const provider = structuredClone(providerCatalog.providers[1]);
    expect(
      () => new ProviderCatalogIndex({ providers: [provider, structuredClone(provider)] }),
    ).toThrow('Duplicate provider definition: codex');

    const duplicateMetric = structuredClone(provider);
    duplicateMetric.metrics.push(structuredClone(duplicateMetric.metrics[0]));
    expect(() => new ProviderCatalogIndex({ providers: [duplicateMetric] })).toThrow(
      'Duplicate metric definition: codex.session',
    );
  });

  it('localizes typed Grok caps and Claude retry notices in all shipped languages', () => {
    const cap: StatusMetric = {
      id: 'payAsYouGo',
      label: 'Extra Usage',
      text: '',
      tone: 'positive',
      value: 12.5,
      unit: 'cap',
    };
    const notice: ProviderNotice = {
      id: 'rateLimited',
      title: 'Live usage paused',
      message: '',
      tone: 'warning',
      retrySeconds: 60,
      showingStaleLimits: true,
    };

    setUiLanguage('en');
    expect(localizedStatusText(cap)).toBe('12.5 cap');
    expect(localizedNoticeTitle(notice)).toBe('Live usage paused');
    expect(localizedNoticeMessage(notice)).toBe(
      'Showing the last successful limits · Retrying in about 1 minute',
    );

    setUiLanguage('zh-CN');
    expect(localizedStatusText(cap)).toBe('上限 12.5');
    expect(localizedNoticeTitle(notice)).toBe('实时用量已暂停');
    expect(localizedNoticeMessage(notice)).toBe('显示上次成功获取的限额 · 约 1 分钟后重试');

    setUiLanguage('zh-TW');
    expect(localizedStatusText(cap)).toBe('上限 12.5');
    expect(localizedNoticeTitle(notice)).toBe('即時用量已暫停');
    expect(localizedNoticeMessage(notice)).toBe('顯示上次成功取得的限額 · 大約 1 分鐘後重試');
  });

  it('keeps untyped and unknown status text untouched', () => {
    const status: StatusMetric = {
      id: 'custom.status',
      label: 'Custom',
      text: 'Provider text',
      tone: 'positive',
    };
    const notice: ProviderNotice = {
      id: 'customNotice',
      title: 'Provider title',
      message: 'Provider message',
      tone: 'info',
    };
    setUiLanguage('zh-TW');
    expect(localizedStatusText(status)).toBe('Provider text');
    expect(localizedNoticeTitle(notice)).toBe('Provider title');
    expect(localizedNoticeMessage(notice)).toBe('Provider message');
  });
});
