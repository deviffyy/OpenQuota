import { describe, expect, it } from 'vitest';
import { providerCatalog } from '../test/appFixtures';
import { ProviderCatalogIndex } from './metrics';

describe('provider catalog index', () => {
  it('indexes provider identity and metric metadata from bootstrap data', () => {
    const catalog = new ProviderCatalogIndex(providerCatalog);

    expect(catalog.displayName('codex')).toBe('Codex');
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
});
