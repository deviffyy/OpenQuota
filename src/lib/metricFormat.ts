import type { AppSettings } from './types';
import { getUiLanguage, localeFor } from './i18n';

export type MetricNumberKind = 'percent' | 'dollars' | 'count';
export type MetricNumberStyle = 'tray' | 'row' | 'full';

function formatters() {
  const locale = localeFor(getUiLanguage());
  return {
    compact: new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 1 }),
    row: new Intl.NumberFormat(locale, { minimumFractionDigits: 0, maximumFractionDigits: 1 }),
    full: new Intl.NumberFormat(locale, { minimumFractionDigits: 0, maximumFractionDigits: 1 }),
    currency: new Intl.NumberFormat(locale, { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 2 }),
    wholeDollar: new Intl.NumberFormat(locale, { style: 'currency', currency: 'USD', minimumFractionDigits: 0, maximumFractionDigits: 0 }),
  };
}

export function formatMetricNumber(
  value: number,
  kind: MetricNumberKind,
  style: MetricNumberStyle,
) {
  if (!Number.isFinite(value)) return '—';
  const formatter = formatters();
  if (kind === 'percent') return `${Math.round(Math.min(100, Math.max(0, value)))}%`;
  if (kind === 'dollars') {
    if (Math.abs(value) >= 1000 && style !== 'full') {
      return `$${formatter.compact.format(value)}`;
    }
    return style === 'tray' ? formatter.wholeDollar.format(value) : formatter.currency.format(value);
  }
  if (style !== 'full' && Math.abs(value) >= 1000) return formatter.compact.format(value);
  return (style === 'full' ? formatter.full : formatter.row).format(value);
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
  const formatter = formatters();
  if (metric === 'cost') {
    return { primary: formatMetricNumber(value, 'dollars', 'tray'), unit: 'dollars' };
  }
  if (metric === 'costPerMillion') {
    return { primary: formatMetricNumber(value, 'dollars', 'row'), unit: 'MTok' };
  }
  const magnitude = Math.abs(value);
  if (magnitude >= 1_000_000_000) {
    return { primary: formatter.row.format(value / 1_000_000_000), unit: 'billion' };
  }
  if (magnitude >= 1_000_000) {
    return { primary: formatter.row.format(value / 1_000_000), unit: 'million' };
  }
  if (magnitude >= 1_000) {
    return { primary: formatter.row.format(value / 1_000), unit: 'thousand' };
  }
  return { primary: formatter.row.format(value), unit: 'tokens' };
}
