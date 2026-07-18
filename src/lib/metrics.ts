import type {
  MetricDefinition,
  ProviderCatalog,
  ProviderDefinition,
  ProviderSnapshot,
} from './types';

export class ProviderCatalogIndex {
  readonly providers: ProviderDefinition[];
  readonly #providersById: Map<string, ProviderDefinition>;
  readonly #metricsById: Map<string, MetricDefinition>;

  constructor(catalog: ProviderCatalog) {
    this.providers = catalog.providers;
    this.#providersById = new Map();
    this.#metricsById = new Map();

    for (const provider of catalog.providers) {
      if (this.#providersById.has(provider.id)) {
        throw new Error(`Duplicate provider definition: ${provider.id}`);
      }
      this.#providersById.set(provider.id, provider);
      for (const metric of provider.metrics) {
        if (this.#metricsById.has(metric.id)) {
          throw new Error(`Duplicate metric definition: ${metric.id}`);
        }
        this.#metricsById.set(metric.id, metric);
      }
    }
  }

  provider(id: string) {
    return this.#providersById.get(id);
  }

  metric(id: string) {
    return this.#metricsById.get(id);
  }

  displayName(id: string) {
    return this.provider(id)?.displayName ?? id;
  }

  supportsSpend(id: string) {
    return this.provider(id)?.metrics.some((metric) => metric.source.kind === 'usage') ?? false;
  }

  localUsageSourceNote(id: string) {
    const provider = this.provider(id);
    return (
      provider?.localUsageSourceNote ?? `From your ${provider?.displayName ?? id} usage history`
    );
  }
}

export function usageSourceNote(catalog: ProviderCatalogIndex, snapshot: ProviderSnapshot) {
  return (
    snapshot.usage.last30Days?.modelBreakdown?.sourceNote ??
    snapshot.usage.today?.modelBreakdown?.sourceNote ??
    snapshot.usage.yesterday?.modelBreakdown?.sourceNote ??
    catalog.localUsageSourceNote(snapshot.providerId)
  );
}

export const emptyProviderCatalog = new ProviderCatalogIndex({ providers: [] });
