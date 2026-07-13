<script lang="ts">
  import { onDestroy } from 'svelte';
  import { SvelteDate } from 'svelte/reactivity';
  import type { DailyUsage } from './types';

  interface Props {
    daily: DailyUsage[];
    sourceNote: string;
  }
  let { daily, sourceNote }: Props = $props();
  const points = $derived(fillDays(daily));
  const max = $derived(Math.max(1, ...points.map((point) => point.tokens)));
  const total = $derived(points.reduce((sum, point) => sum + point.tokens, 0));
  const peak = $derived(
    points.reduce((best, point) => (point.tokens > best.tokens ? point : best), points[0]),
  );
  let detailVisible = $state(false);
  let hoveredDate = $state<string | null>(null);
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;
  const highlightedPoint = $derived(
    hoveredDate === null ? peak : (points.find((point) => point.date === hoveredDate) ?? peak),
  );

  function fillDays(entries: DailyUsage[]) {
    const byDate = new Map(entries.map((entry) => [entry.date, entry]));
    const result: DailyUsage[] = [];
    const today = new SvelteDate();
    today.setHours(12, 0, 0, 0);
    for (let offset = 29; offset >= 0; offset -= 1) {
      const date = new SvelteDate(today);
      date.setDate(today.getDate() - offset);
      const key = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
      result.push(
        byDate.get(key) ?? { date: key, tokens: 0, estimatedCostUsd: null, estimateComplete: true },
      );
    }
    return result;
  }

  function compact(value: number) {
    return new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(
      value,
    );
  }

  function dayLabel(value: string) {
    const date = new Date(`${value}T12:00:00`);
    return Number.isNaN(date.getTime())
      ? value
      : new Intl.DateTimeFormat('en-US', { month: 'short', day: 'numeric' }).format(date);
  }

  function revealDetail() {
    if (hoverTimer) clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => (detailVisible = true), 400);
  }

  function concealDetail() {
    if (hoverTimer) clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => (detailVisible = false), 180);
  }

  onDestroy(() => {
    if (hoverTimer) clearTimeout(hoverTimer);
  });
</script>

<section class="trend-row" aria-label="Usage Trend">
  <strong>Usage Trend</strong>
  {#if total > 0}
    <div
      class="trend-chart-wrap"
      role="group"
      aria-label="Usage trend chart details"
      onmouseenter={revealDetail}
      onmouseleave={concealDetail}
    >
      <div
        class="trend-bars"
        class:trend-bars--active={detailVisible}
        role="img"
        aria-label={`30-day token chart. Peak ${compact(peak.tokens)} tokens on ${peak.date}.`}
      >
        {#each points as point (point.date)}
          <span
            style={`height: ${Math.max(point.tokens > 0 ? 18 : 2, (point.tokens / max) * 100)}%`}
            title={`${point.date}: ${compact(point.tokens)} tokens`}
          ></span>
        {/each}
      </div>
      {#if detailVisible}
        <aside class="trend-detail" onmouseenter={revealDetail} onmouseleave={concealDetail}>
          <header>
            <strong>Usage Trend</strong><span
              >{hoveredDate
                ? `${dayLabel(highlightedPoint.date)} · ${compact(highlightedPoint.tokens)} tokens`
                : `peak ${compact(peak.tokens)} tokens`}</span
            >
          </header>
          <div
            class="trend-detail__bars"
            aria-hidden="true"
            onmouseleave={() => (hoveredDate = null)}
          >
            {#each points as point (point.date)}
              <i
                role="presentation"
                class:muted={hoveredDate !== null && hoveredDate !== point.date}
                style={`height: ${Math.max(point.tokens > 0 ? 8 : 2, (point.tokens / max) * 100)}%`}
                onmouseenter={() => (hoveredDate = point.date)}
              ></i>
            {/each}
          </div>
          <div class="trend-detail__axis">
            <span>{dayLabel(points[0]?.date ?? '')}</span><span
              >{dayLabel(points.at(-1)?.date ?? '')}</span
            >
          </div>
          <small class="trend-detail__source">{sourceNote}</small>
        </aside>
      {/if}
    </div>
  {:else}
    <p class="trend-empty">No data</p>
  {/if}
</section>

<style>
  :global {
    .trend-empty {
      display: flex;
      width: clamp(90px, 48%, 150px);
      min-height: 18px;
      align-items: center;
      justify-content: flex-end;
      margin: 0;
      color: var(--secondary);
      font-size: 10px;
      text-align: right;
    }

    .trend-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      padding: 9px 14px 10px;
    }

    .trend-row > strong {
      flex: 0 0 auto;
      font-size: 12px;
      font-weight: 600;
    }

    .trend-chart-wrap {
      position: relative;
      width: clamp(90px, 48%, 150px);
      flex: 1;
    }

    .trend-bars {
      display: flex;
      width: 100%;
      height: 18px;
      align-items: flex-end;
      gap: 1.5px;
      margin: 0;
      padding: 0;
      border-radius: 6px;
    }

    .trend-bars span {
      min-height: 2px;
      flex: 1;
      border-radius: 1px;
      background: var(--meter-fill);
      transition:
        height var(--motion-switch),
        opacity var(--motion-switch),
        transform var(--motion-switch);
    }

    .trend-bars:hover span {
      opacity: 0.55;
    }

    .trend-bars span:hover {
      opacity: 1;
      transform: scaleX(1.35);
    }

    .trend-bars--active {
      background: var(--button-hover);
      box-shadow: 0 0 0 4px var(--button-hover);
    }

    .trend-detail {
      position: absolute;
      right: -8px;
      bottom: calc(100% + 12px);
      z-index: 20;
      display: grid;
      width: 240px;
      gap: 8px;
      padding: 12px;
      border: 1px solid var(--separator);
      border-radius: 12px;
      color: var(--text);
      background: color-mix(in srgb, var(--tray) 96%, transparent);
      box-shadow: 0 12px 36px rgba(0, 0, 0, 0.28);
      backdrop-filter: blur(20px);
      animation: detail-in 180ms ease-out both;
    }

    .trend-detail > header,
    .trend-detail__axis {
      display: flex;
      align-items: baseline;
      justify-content: space-between;
      gap: 8px;
    }

    .trend-detail > header strong {
      font-size: 13px;
      font-weight: 600;
    }

    .trend-detail > header span {
      color: var(--secondary);
      font-size: 11px;
      font-variant-numeric: tabular-nums;
      white-space: nowrap;
    }

    .trend-detail__bars {
      display: flex;
      height: 76px;
      align-items: flex-end;
      gap: 2px;
      margin: 0;
    }

    .trend-detail__bars i {
      min-height: 2px;
      flex: 1;
      border-radius: 1px;
      background: var(--meter-fill);
      transition: opacity 120ms ease;
    }

    .trend-detail__bars i.muted {
      opacity: 0.28;
    }

    .trend-detail__axis {
      color: var(--secondary);
      font-size: 10px;
      font-variant-numeric: tabular-nums;
    }

    .trend-detail__source {
      color: var(--secondary);
      font-size: 10px;
      text-align: center;
    }

    :root[data-density='compact'] .trend-row {
      padding: 6px 14px;
    }

    :root[data-density='compact'] .trend-bars {
      height: 14px;
    }

    :root[data-density='compact'] .trend-empty {
      min-height: 14px;
    }
  }
</style>
