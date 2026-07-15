<script lang="ts">
  import { formatMetricValue } from './metricFormat';
  import { formatReset } from './pacing';
  import type { ValueMetric } from './types';

  interface Props {
    label: string;
    metric: ValueMetric | null;
    now: number;
    resetDisplay: 'countdown' | 'exact';
    timeFormat: 'system' | 'twelveHour' | 'twentyFourHour';
  }

  let { label, metric, now, resetDisplay, timeFormat }: Props = $props();

  const reading = $derived(
    metric?.values
      .map((value) => formatMetricValue(value.number, value.kind, 'row', value.label ?? undefined))
      .join(' · ') ?? 'No data',
  );
  const tooltip = $derived.by(() => {
    if (!metric) return undefined;
    if (metric.expiriesAt.length) {
      const sorted = [...metric.expiriesAt].sort();
      const lines = sorted.map((expiry, index) => {
        const formatted = formatReset(expiry, now, resetDisplay, timeFormat).replace(
          /^Resets(?: in)?\s*/,
          '',
        );
        return `${index + 1}. ${formatted}`;
      });
      return [resetDisplay === 'countdown' ? 'Resets expire in:' : 'Resets expire:', ...lines].join(
        '\n',
      );
    }
    const count = metric.values[0]?.number ?? 0;
    if (metric.id === 'rateLimitResets' && count > 0) return 'Expiry times unavailable';
    if (metric.values.some((value) => Math.abs(value.number) >= 1000)) {
      return metric.values
        .map((value) =>
          formatMetricValue(value.number, value.kind, 'full', value.label ?? undefined),
        )
        .join(' · ');
    }
    return undefined;
  });
  const expirySeverity = $derived.by(() => {
    if (!metric?.expiriesAt.length) return null;
    const remaining =
      Math.min(...metric.expiriesAt.map((value) => new Date(value).getTime())) - now;
    if (remaining <= 48 * 60 * 60 * 1000) return 'critical';
    if (remaining <= 7 * 24 * 60 * 60 * 1000) return 'warning';
    return 'normal';
  });
</script>

<div class="value-row">
  <span>{label}</span>
  <span class="value-reading" data-tooltip={tooltip}>
    {#if expirySeverity}<i class="expiry-dot expiry-dot--{expirySeverity}" aria-hidden="true"
      ></i>{/if}
    {reading}
  </span>
</div>

<style>
  :global {
    .value-row {
      display: flex;
      align-items: baseline;
      justify-content: space-between;
      gap: 8px;
      padding: 6px 14px;
      font-size: 12px;
      line-height: 15px;
    }

    .value-row > span:first-child {
      color: color-mix(in srgb, var(--text) 88%, var(--card));
      font-weight: 600;
    }

    .value-reading {
      display: inline-flex;
      align-items: center;
      gap: 5px;
      text-align: right;
    }

    .expiry-dot {
      width: 6px;
      height: 6px;
      border-radius: 50%;
      background: var(--meter-fill);
    }

    .expiry-dot--warning {
      background: var(--meter-warning);
    }

    .expiry-dot--critical {
      background: var(--meter-critical);
    }

    :root[data-density='compact'] .value-row {
      padding: 3px 14px;
      font-size: 11px;
    }
  }
</style>
