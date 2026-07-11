import type { AppSettings, UsageHistory, UsagePeriod } from './types';

export interface SpendProvider {
  id: string;
  usage: UsageHistory;
}

export interface SpendSlice {
  id: string;
  value: number;
  period: UsagePeriod;
}

export interface SpendProjection {
  slices: SpendSlice[];
  centerValue: number | null;
  estimateComplete: boolean;
}

export function selectedPeriod(
  history: UsageHistory,
  selection: AppSettings['totalSpendPeriod'],
): UsagePeriod | null {
  if (selection === 'today') return history.today;
  if (selection === 'yesterday') return history.yesterday;
  return history.last30Days;
}

function valueFor(period: UsagePeriod, metric: AppSettings['totalSpendMetric']): number | null {
  if (metric === 'tokens') return period.tokens > 0 ? period.tokens : null;
  if (period.estimatedCostUsd === null || period.estimatedCostUsd <= 0) return null;
  if (metric === 'costPerMillion') {
    return period.tokens > 0 ? (period.estimatedCostUsd / period.tokens) * 1_000_000 : null;
  }
  return period.estimatedCostUsd;
}

export function projectSpend(
  providers: SpendProvider[],
  periodSelection: AppSettings['totalSpendPeriod'],
  metric: AppSettings['totalSpendMetric'],
): SpendProjection {
  const slices = providers
    .flatMap((provider) => {
      const period = selectedPeriod(provider.usage, periodSelection);
      if (!period) return [];
      const value = valueFor(period, metric);
      return value === null ? [] : [{ id: provider.id, value, period }];
    })
    .sort((left, right) => right.value - left.value || left.id.localeCompare(right.id));

  if (slices.length === 0) {
    return { slices, centerValue: null, estimateComplete: true };
  }

  const centerValue =
    metric === 'costPerMillion'
      ? (() => {
          const totalCost = slices.reduce(
            (sum, slice) => sum + (slice.period.estimatedCostUsd ?? 0),
            0,
          );
          const totalTokens = slices.reduce((sum, slice) => sum + slice.period.tokens, 0);
          return totalTokens > 0 ? (totalCost / totalTokens) * 1_000_000 : null;
        })()
      : slices.reduce((sum, slice) => sum + slice.value, 0);

  return {
    slices,
    centerValue,
    estimateComplete: slices.every((slice) => slice.period.estimateComplete),
  };
}

export function emptySpendMessage(metric: AppSettings['totalSpendMetric']) {
  if (metric === 'tokens') return 'No token data for this period';
  if (metric === 'costPerMillion') return 'No cost-per-token data for this period';
  return 'No cost data for this period';
}
