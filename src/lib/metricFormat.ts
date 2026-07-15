import type { AppSettings } from './types';

export type MetricNumberKind = 'percent' | 'dollars' | 'count';
export type MetricNumberStyle = 'tray' | 'row' | 'full';

const compactFormatter = new Intl.NumberFormat('en-US', {
  notation: 'compact',
  maximumFractionDigits: 1,
});
const rowNumberFormatter = new Intl.NumberFormat('en-US', {
  minimumFractionDigits: 0,
  maximumFractionDigits: 1,
});
const fullNumberFormatter = new Intl.NumberFormat('en-US', {
  minimumFractionDigits: 0,
  maximumFractionDigits: 1,
});
const currencyFormatter = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});
const wholeDollarFormatter = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 0,
  maximumFractionDigits: 0,
});

export function formatMetricNumber(
  value: number,
  kind: MetricNumberKind,
  style: MetricNumberStyle,
) {
  if (!Number.isFinite(value)) return '—';
  if (kind === 'percent') return `${Math.round(Math.min(100, Math.max(0, value)))}%`;
  if (kind === 'dollars') {
    if (Math.abs(value) >= 1000 && style !== 'full') {
      return `$${compactFormatter.format(value)}`;
    }
    return style === 'tray' ? wholeDollarFormatter.format(value) : currencyFormatter.format(value);
  }
  if (style !== 'full' && Math.abs(value) >= 1000) return compactFormatter.format(value);
  return (style === 'full' ? fullNumberFormatter : rowNumberFormatter).format(value);
}

export function formatMetricValue(
  value: number,
  kind: MetricNumberKind,
  style: MetricNumberStyle,
  label?: string,
) {
  const formatted = formatMetricNumber(value, kind, style);
  return label ? `${formatted} ${label}` : formatted;
}

export function formatSpendValue(
  value: number,
  metric: AppSettings['totalSpendMetric'],
  style: MetricNumberStyle = 'row',
) {
  if (metric === 'tokens') return formatMetricNumber(value, 'count', style);
  const dollars = formatMetricNumber(value, 'dollars', style);
  return metric === 'costPerMillion' ? `${dollars}/MTok` : dollars;
}

export function totalSpendRingCenter(value: number, metric: AppSettings['totalSpendMetric']) {
  if (metric === 'cost') {
    return { primary: formatMetricNumber(value, 'dollars', 'tray'), unit: 'dollars' };
  }
  if (metric === 'costPerMillion') {
    return { primary: formatMetricNumber(value, 'dollars', 'row'), unit: 'MTok' };
  }
  const magnitude = Math.abs(value);
  if (magnitude >= 1_000_000_000) {
    return { primary: rowNumberFormatter.format(value / 1_000_000_000), unit: 'billion' };
  }
  if (magnitude >= 1_000_000) {
    return { primary: rowNumberFormatter.format(value / 1_000_000), unit: 'million' };
  }
  if (magnitude >= 1_000) {
    return { primary: rowNumberFormatter.format(value / 1_000), unit: 'thousand' };
  }
  return { primary: rowNumberFormatter.format(value), unit: 'tokens' };
}
