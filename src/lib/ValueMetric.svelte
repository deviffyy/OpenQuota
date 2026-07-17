<script lang="ts">
  import { onDestroy } from 'svelte';
  import { formatMetricValue } from './metricFormat';
  import { formatReset } from './pacing';
  import ResetCreditsDetail from './ResetCreditsDetail.svelte';
  import type { ValueMetric } from './types';

  interface Props {
    label: string;
    metric: ValueMetric | null;
    now: number;
    resetDisplay: 'countdown' | 'exact';
    timeFormat: 'system' | 'twelveHour' | 'twentyFourHour';
  }

  let { label, metric, now, resetDisplay, timeFormat }: Props = $props();
  let detailOpen = $state(false);
  let detailTop = $state(8);
  let showTimer: ReturnType<typeof setTimeout> | undefined;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  const showsResetDetail = $derived(metric?.id === 'rateLimitResets');

  const reading = $derived(
    metric?.values
      .map((value) => formatMetricValue(value.number, value.kind, 'row', value.label ?? undefined))
      .join(' · ') ?? 'No data',
  );
  const tooltip = $derived.by(() => {
    if (!metric) return undefined;
    if (metric.expiriesAt.length && !showsResetDetail) {
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

  function scheduleShow(event: Event) {
    if (!showsResetDetail || detailOpen || showTimer) return;
    if (hideTimer) clearTimeout(hideTimer);
    const target = event.currentTarget as HTMLElement;
    const rect = target.getBoundingClientRect();
    const estimatedHeight = Math.min(72 + (metric?.expiriesAt.length ?? 0) * 34, 340);
    detailTop = Math.max(8, Math.min(rect.bottom + 7, window.innerHeight - estimatedHeight - 8));
    showTimer = setTimeout(() => {
      detailOpen = true;
      showTimer = undefined;
    }, 350);
  }
  function scheduleHide() {
    if (showTimer) clearTimeout(showTimer);
    showTimer = undefined;
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      detailOpen = false;
      hideTimer = undefined;
    }, 180);
  }
  function keepOpen() {
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = undefined;
  }
  function toggleDetail(event: Event) {
    if (!showsResetDetail) return;
    if (detailOpen) {
      detailOpen = false;
      return;
    }
    scheduleShow(event);
    if (showTimer) clearTimeout(showTimer);
    showTimer = undefined;
    detailOpen = true;
  }
  onDestroy(() => {
    if (showTimer) clearTimeout(showTimer);
    if (hideTimer) clearTimeout(hideTimer);
  });
</script>

<div class="value-row">
  <span>{label}</span>
  {#if showsResetDetail}
    <button
      type="button"
      class="value-reading value-reading--interactive"
      aria-expanded={detailOpen}
      aria-label={`${label}: ${reading}`}
      onmouseenter={scheduleShow}
      onmouseleave={scheduleHide}
      onfocus={scheduleShow}
      onblur={scheduleHide}
      onclick={toggleDetail}
    >
      {#if expirySeverity}<i class="expiry-dot expiry-dot--{expirySeverity}" aria-hidden="true"
        ></i>{/if}
      {reading}
    </button>
  {:else}
    <span class="value-reading" data-tooltip={tooltip}>{reading}</span>
  {/if}
</div>

{#if detailOpen && metric}
  <ResetCreditsDetail
    title={label}
    count={Math.max(0, Math.floor(metric.values[0]?.number ?? 0))}
    expiries={metric.expiriesAt}
    {now}
    {timeFormat}
    top={detailTop}
    onEnter={keepOpen}
    onLeave={scheduleHide}
    onDismiss={() => (detailOpen = false)}
  />
{/if}

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

    .value-reading--interactive {
      padding: 0;
      border: 0;
      border-radius: 6px;
      color: inherit;
      background: none;
      font: inherit;
      cursor: default;
      outline: none;
      transition:
        background-color 120ms ease,
        box-shadow 120ms ease;
    }

    .value-reading--interactive:hover,
    .value-reading--interactive:focus-visible,
    .value-reading--interactive[aria-expanded='true'] {
      background: var(--button-hover);
      box-shadow: 0 0 0 4px var(--button-hover);
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
