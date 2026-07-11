<script lang="ts">
  import { SvelteDate } from 'svelte/reactivity';
  import type { DailyUsage } from './types';

  interface Props {
    daily: DailyUsage[];
  }
  let { daily }: Props = $props();
  const points = $derived(fillDays(daily));
  const max = $derived(Math.max(1, ...points.map((point) => point.tokens)));
  const total = $derived(points.reduce((sum, point) => sum + point.tokens, 0));
  const peak = $derived(
    points.reduce((best, point) => (point.tokens > best.tokens ? point : best), points[0]),
  );
  let detailVisible = $state(false);
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;

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

  function revealDetail() {
    if (hoverTimer) clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => (detailVisible = true), 400);
  }

  function concealDetail() {
    if (hoverTimer) clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => (detailVisible = false), 180);
  }
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
          <strong>Usage Trend</strong>
          <span>{points[0]?.date} – {points.at(-1)?.date}</span>
          <div class="trend-detail__bars" aria-hidden="true">
            {#each points as point (point.date)}
              <i
                style={`height: ${Math.max(point.tokens > 0 ? 8 : 2, (point.tokens / max) * 100)}%`}
              ></i>
            {/each}
          </div>
          <p><b>Peak {compact(peak.tokens)}</b><small>{compact(total)} tokens total</small></p>
        </aside>
      {/if}
    </div>
  {:else}
    <p class="trend-empty">No local usage</p>
  {/if}
</section>
