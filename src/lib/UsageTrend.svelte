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
