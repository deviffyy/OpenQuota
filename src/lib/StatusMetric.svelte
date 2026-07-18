<script lang="ts">
  import type { StatusMetric as StatusMetricValue } from './types';

  interface Props {
    label: string;
    metric: StatusMetricValue | null;
  }

  let { label, metric }: Props = $props();
</script>

<div class="status-row">
  <span>{label}</span>
  {#if metric}
    <span
      class="status-badge status-badge--{metric.tone}"
      data-tooltip={metric.subtitle ?? undefined}
    >
      {metric.text}
    </span>
  {:else}
    <span class="status-reading">No data</span>
  {/if}
</div>

<style>
  :global {
    .status-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      padding: 6px 14px;
      font-size: 12px;
      line-height: 15px;
    }

    .status-row > span:first-child {
      color: color-mix(in srgb, var(--text) 88%, var(--card));
      font-weight: 600;
    }

    .status-badge {
      max-width: 58%;
      overflow: hidden;
      padding: 2px 7px;
      border-radius: 999px;
      color: var(--secondary);
      background: var(--button-hover);
      font-weight: 600;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .status-badge--positive {
      color: var(--meter-fill);
      background: color-mix(in srgb, var(--meter-fill) 12%, transparent);
    }

    .status-badge--warning {
      color: var(--meter-warning);
      background: color-mix(in srgb, var(--meter-warning) 12%, transparent);
    }

    .status-badge--danger {
      color: var(--meter-critical);
      background: color-mix(in srgb, var(--meter-critical) 12%, transparent);
    }

    .status-reading {
      color: var(--secondary);
    }

    :root[data-density='compact'] .status-row {
      padding: 3px 14px;
      font-size: 11px;
    }
  }
</style>
