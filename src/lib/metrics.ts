import type {
  MetricDefinition,
  MetricLabelKind,
  ProviderCatalog,
  ProviderDefinition,
  ProviderLink,
  ProviderLinkKind,
  ProviderNotice,
  ProviderSnapshot,
  StatusMetric,
  UsageSourceKind,
} from './types';
import { t, type TranslationKey } from './i18n';

const metricLabelKeys: Record<MetricLabelKind, TranslationKey> = {
  session: 'session',
  weekly: 'weekly',
  today: 'today',
  yesterday: 'yesterday',
  last30Days: 'last30Days',
  daily: 'daily',
  monthly: 'monthly',
  usageTrend: 'usageTrend',
  extraUsage: 'extraUsage',
  extraBalance: 'extraBalance',
  rateLimitResets: 'rateLimitResets',
  credits: 'credits',
  totalUsage: 'totalUsage',
  autoUsage: 'autoUsage',
  apiUsage: 'apiUsage',
  requests: 'requestsLabel',
  balance: 'balance',
  thisWeek: 'thisWeek',
  thisMonth: 'thisMonth',
  keyLimit: 'keyLimit',
  webSearches: 'webSearches',
  sparkWeekly: 'sparkWeekly',
  claudeWeekly: 'claudeWeekly',
  orgCredits: 'orgCredits',
  orgSpend: 'orgSpend',
  chat: 'chat',
  completions: 'completions',
};

const usageSourceKeys: Partial<Record<UsageSourceKind, TranslationKey>> = {
  estimatedLogs: 'estimatedLogsSource',
  estimatedLogsWithPi: 'estimatedLogsWithPiSource',
  estimatedUsageHistory: 'estimatedUsageHistorySource',
  estimatedHistoryWithPi: 'estimatedHistoryWithPiSource',
  cursorExport: 'cursorExportSource',
  openCodeDatabase: 'openCodeDatabaseSource',
};

const providerLinkKeys: Record<ProviderLinkKind, TranslationKey> = {
  status: 'providerLinkStatus',
  dashboard: 'providerLinkDashboard',
  apiKeys: 'providerLinkApiKeys',
  usage: 'providerLinkUsage',
  activity: 'providerLinkActivity',
  credits: 'providerLinkCredits',
};

function localizedUsageSource(kind: UsageSourceKind | null | undefined, provider: string) {
  const key = kind && usageSourceKeys[kind];
  return key ? t(key, { provider }) : undefined;
}

function localizedMetricLabel(definition: MetricDefinition) {
  return definition.labelKind ? t(metricLabelKeys[definition.labelKind]) : definition.label;
}

export function localizedProviderLinkLabel(link: ProviderLink) {
  return link.kind ? t(providerLinkKeys[link.kind]) : link.label;
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
      localizedUsageSource(provider?.localUsageSourceKind, name) ??
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
  const source = localizedUsageSource(
    breakdown?.sourceKind,
    catalog.displayName(snapshot.providerId),
  );
  if (source) return source;
  if (breakdown?.sourceNote.trim()) return breakdown.sourceNote;
  return catalog.localUsageSourceNote(snapshot.providerId);
}

export const emptyProviderCatalog = new ProviderCatalogIndex({ providers: [] });
