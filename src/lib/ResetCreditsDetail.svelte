<script lang="ts">
  import { formatReset } from './pacing';

  interface Props {
    title: string;
    count: number;
    expiries: string[];
    now: number;
    timeFormat: 'system' | 'twelveHour' | 'twentyFourHour';
    top: number;
    onEnter: () => void;
    onLeave: () => void;
  }

  let { title, count, expiries, now, timeFormat, top, onEnter, onLeave }: Props = $props();

  const entries = $derived(
    [...expiries].sort().map((expiry, index) => {
      const timestamp = new Date(expiry).getTime();
      const remaining = timestamp - now;
      const severity =
        remaining <= 48 * 60 * 60 * 1000
          ? 'critical'
          : remaining <= 7 * 24 * 60 * 60 * 1000
            ? 'warning'
            : 'normal';
      const relative = formatReset(expiry, now, 'countdown', timeFormat).replace(
        /^Resets(?: in)?\s*/,
        '',
      );
      const exact = formatReset(expiry, now, 'exact', timeFormat).replace(/^Resets\s*/, '');
      const imminent = relative === 'soon';
      return {
        id: `${expiry}:${index}`,
        number: index + 1,
        severity,
        exact: imminent ? 'Expiring soon' : exact,
        relative: imminent ? null : relative,
      };
    }),
  );
</script>

<div
  class="reset-credits-detail"
  style={`top:${top}px`}
  role="tooltip"
  aria-label={`${title} expiry details`}
  onmouseenter={onEnter}
  onmouseleave={onLeave}
>
  <h3>{title}</h3>
  {#if entries.length > 0}
    <div class="reset-timeline">
      {#each entries as entry, index (entry.id)}
        <div class="reset-entry">
          <div class="reset-rail" aria-hidden="true">
            <i class:reset-rail-hidden={index === 0}></i>
            <b class="reset-node reset-node--{entry.severity}">{entry.number}</b>
            <i class:reset-rail-hidden={index === entries.length - 1}></i>
          </div>
          <span>{entry.exact}</span>
          {#if entry.relative}<small>{entry.relative}</small>{/if}
        </div>
      {/each}
    </div>
  {:else if count > 0}
    <div class="reset-empty">
      <strong>{count} available</strong>
      <span>Expiry times unavailable</span>
    </div>
  {:else}
    <div class="reset-empty"><span>No rate limit resets available</span></div>
  {/if}
</div>

<style>
  :global {
    .reset-credits-detail {
      position: fixed;
      right: 8px;
      z-index: 100;
      box-sizing: border-box;
      width: 268px;
      max-height: calc(100vh - 16px);
      padding: 14px;
      overflow-y: auto;
      border: 1px solid var(--separator);
      border-radius: 12px;
      color: var(--text);
      background: color-mix(in srgb, var(--tray) 97%, transparent);
      box-shadow: 0 12px 36px rgba(0, 0, 0, 0.28);
      animation: reset-detail-in 120ms ease-out both;
    }

    .reset-credits-detail h3 {
      margin: 0 0 8px;
      font-size: 13px;
      font-weight: 650;
      line-height: 17px;
    }

    .reset-timeline {
      display: grid;
    }

    .reset-entry {
      display: grid;
      min-height: 34px;
      grid-template-columns: 20px minmax(0, 1fr) auto;
      align-items: center;
      gap: 8px;
      font-size: 11px;
      line-height: 14px;
    }

    .reset-entry > span {
      overflow: hidden;
      font-weight: 550;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .reset-entry > small {
      color: var(--secondary);
      font: inherit;
      font-variant-numeric: tabular-nums;
      white-space: nowrap;
    }

    .reset-rail {
      display: grid;
      height: 100%;
      grid-template-rows: 1fr 18px 1fr;
      justify-items: center;
      align-items: center;
    }

    .reset-rail > i {
      width: 1px;
      height: 100%;
      background: var(--separator);
    }

    .reset-rail > .reset-rail-hidden {
      opacity: 0;
    }

    .reset-node {
      display: grid;
      width: 18px;
      height: 18px;
      place-items: center;
      border-radius: 50%;
      color: white;
      background: var(--meter-fill);
      font-size: 10px;
      font-weight: 650;
      line-height: 1;
    }

    .reset-node--warning {
      color: #171719;
      background: var(--meter-warning);
    }

    .reset-node--critical {
      background: var(--meter-critical);
    }

    .reset-empty {
      display: grid;
      justify-items: center;
      gap: 4px;
      padding: 12px 4px 8px;
      color: var(--secondary);
      font-size: 11px;
      line-height: 15px;
      text-align: center;
    }

    .reset-empty strong {
      color: var(--text);
      font-weight: 650;
    }

    @keyframes reset-detail-in {
      from {
        opacity: 0;
        transform: translateY(-2px) scale(0.98);
      }
      to {
        opacity: 1;
        transform: translateY(0) scale(1);
      }
    }

    :root[data-density='compact'] .reset-credits-detail h3 {
      font-size: 12px;
    }

    :root[data-density='compact'] .reset-entry {
      min-height: 30px;
      font-size: 10px;
    }
  }
</style>
