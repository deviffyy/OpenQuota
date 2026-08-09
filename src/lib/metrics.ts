import type {
  MetricDefinition,
  ProviderCatalog,
  ProviderDefinition,
  ProviderNotice,
  ProviderSnapshot,
  StatusMetric,
} from './types';
import { messages, t, type TranslationKey } from './i18n';

function localizedKey(key: string | null | undefined, values: Record<string, string | number>) {
  if (!key || !(key in messages.en)) return undefined;
  return t(key as TranslationKey, values);
}

function localizedMetricLabel(definition: MetricDefinition) {
  return localizedKey(definition.labelKey, {}) ?? definition.label;
}

export function localizedStatusText(metric: StatusMetric) {
  if (metric.id !== 'payAsYouGo') return metric.text;
  if (metric.tone === 'neutral') return t('disabled');
  if (metric.tone === 'positive' && metric.unit === 'cap' && Number.isFinite(metric.value)) {
    return t('payAsYouGoCap', { value: String(metric.value) });
  }
  return metric.text;
}

export function localizedNoticeTitle(notice: ProviderNotice) {
  return notice.id === 'rateLimited' && notice.tone === 'warning'
    ? t('liveUsagePaused')
    : notice.title;
}

function localizedRetryMessage(seconds: number) {
  const count = Math.ceil(Math.max(0, seconds) / 60);
  if (count === 0) return t('rateLimitReadyToRetry');
  return count === 1 ? t('rateLimitRetryingMinute') : t('rateLimitRetryingMinutes', { count });
}

export function localizedNoticeMessage(notice: ProviderNotice) {
  if (
    notice.id !== 'rateLimited' ||
    notice.tone !== 'warning' ||
    !Number.isFinite(notice.retrySeconds)
  ) {
    return notice.message;
  }
  const retry = localizedRetryMessage(notice.retrySeconds!);
  return notice.showingStaleLimits === true ? t('rateLimitStaleLimits', { retry }) : retry;
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

  displayName(id: string, providerNames?: Record<string, string>) {
    const customName = providerNames?.[id]?.trim();
    if (customName) return customName;
    return this.provider(id)?.displayName ?? id;
  }

  supportsSpend(id: string) {
    return this.provider(id)?.metrics.some((metric) => metric.source.kind === 'usage') ?? false;
  }

  localUsageSourceNote(id: string) {
    const provider = this.provider(id);
    const name = provider?.displayName ?? id;
    return (
      localizedKey(provider?.localUsageSourceKey, { provider: name }) ??
      provider?.localUsageSourceNote ??
      t('usageHistorySource', { provider: name })
    );
  }
}

export function usageSourceNote(catalog: ProviderCatalogIndex, snapshot: ProviderSnapshot) {
  const breakdown =
    snapshot.usage.last30Days?.modelBreakdown ??
    snapshot.usage.today?.modelBreakdown ??
    snapshot.usage.yesterday?.modelBreakdown;
  const source = localizedKey(breakdown?.sourceKey, {
    provider: catalog.displayName(snapshot.providerId),
  });
  if (source) return source;
  return catalog.localUsageSourceNote(snapshot.providerId);
}

export const emptyProviderCatalog = new ProviderCatalogIndex({ providers: [] });
