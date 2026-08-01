import type {
  MetricDefinition,
  ProviderCatalog,
  ProviderDefinition,
  ProviderSnapshot,
} from './types';
import { t } from './i18n';

function localizedMetricLabel(definition: MetricDefinition) {
  const key = definition.labelKey as Parameters<typeof t>[0] | null | undefined;
  return key ? t(key) : definition.label;
}

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
    const metric = this.#metricsById.get(id);
    return metric ? { ...metric, label: localizedMetricLabel(metric) } : undefined;
  }

  displayName(id: string) {
    return this.provider(id)?.displayName ?? id;
  }

  supportsSpend(id: string) {
    return this.provider(id)?.metrics.some((metric) => metric.source.kind === 'usage') ?? false;
  }

  localUsageSourceNote(id: string) {
    const provider = this.provider(id);
    const name = provider?.displayName ?? id;
    const key = provider?.localUsageSourceKey as Parameters<typeof t>[0] | null | undefined;
    return key
      ? t(key, { provider: name })
      : (provider?.localUsageSourceNote ?? t('usageHistorySource', { provider: name }));
  }
}

export function usageSourceNote(catalog: ProviderCatalogIndex, snapshot: ProviderSnapshot) {
  const source =
    snapshot.usage.last30Days?.modelBreakdown?.sourceNote ??
    snapshot.usage.today?.modelBreakdown?.sourceNote ??
    snapshot.usage.yesterday?.modelBreakdown?.sourceNote;
  const name = catalog.displayName(snapshot.providerId);
  if (source?.includes(' and pi ')) {
    const key = catalog.provider(snapshot.providerId)?.piUsageSourceKey as
      Parameters<typeof t>[0] | null | undefined;
    return t(key ?? 'estimatedLogsWithPiSource', { provider: name });
  }
  return catalog.localUsageSourceNote(snapshot.providerId);
}

export const emptyProviderCatalog = new ProviderCatalogIndex({ providers: [] });
