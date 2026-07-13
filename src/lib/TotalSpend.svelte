<script lang="ts">
  import { onDestroy } from 'svelte';
  import Icon from './Icon.svelte';
  import SelectMenu from './SelectMenu.svelte';
  import { providerDisplayName } from './metrics';
  import { emptySpendMessage, projectSpend, type SpendProjection } from './totalSpend';
  import type { AppSettings, UsageHistory } from './types';

  interface Props {
    providers: Array<{ id: string; usage: UsageHistory }>;
    settings: AppSettings;
    onChange: (settings: AppSettings) => void;
    onShare: (projection: SpendProjection) => boolean | Promise<boolean>;
  }
  let { providers, settings, onChange, onShare }: Props = $props();
  const projection = $derived(
    projectSpend(providers, settings.totalSpendPeriod, settings.totalSpendMetric),
  );
  const providerNames = $derived(providers.map((provider) => providerDisplayName(provider.id)));
  const ringSegments = $derived.by(() => {
    const total = projection.slices.reduce((sum, provider) => sum + provider.value, 0);
    if (total <= 0) return [];
    const floored = projection.slices.map((provider) => Math.max(provider.value / total, 0.025));
    const flooredTotal = floored.reduce((sum, share) => sum + share, 0);
    let cursor = 0;
    return projection.slices.map((provider, index) => {
      const share = (floored[index] / flooredTotal) * 100;
      const gap = projection.slices.length === 1 ? 0 : Math.min(1.2, share * 0.15);
      const segment = {
        id: provider.id,
        length: Math.max(0.5, share - gap),
        offset: cursor + gap / 2,
      };
      cursor += share;
      return segment;
    });
  });
  const periodIndex = $derived(
    settings.totalSpendPeriod === 'today' ? 0 : settings.totalSpendPeriod === 'yesterday' ? 1 : 2,
  );
  let shareCopied = $state(false);
  let shareTimer: ReturnType<typeof setTimeout> | undefined;

  onDestroy(() => {
    if (shareTimer) clearTimeout(shareTimer);
  });

  function compact(value: number) {
    return new Intl.NumberFormat('en-US', {
      notation: 'compact',
      maximumFractionDigits: 1,
    }).format(value);
  }
  function display(value: number | null) {
    if (value === null) return '—';
    if (settings.totalSpendMetric === 'tokens') return compact(value);
    const dollars = `$${value >= 1000 ? compact(value) : value.toFixed(2)}`;
    return settings.totalSpendMetric === 'costPerMillion' ? `${dollars}/MTok` : dollars;
  }
  function ringCenter(value: number | null) {
    if (value === null) return { primary: '—', unit: '' };
    if (settings.totalSpendMetric === 'cost') {
      return {
        primary: value >= 1000 ? `$${compact(value)}` : `$${value.toFixed(0)}`,
        unit: 'dollars',
      };
    }
    if (settings.totalSpendMetric === 'costPerMillion') {
      return {
        primary: value >= 1000 ? `$${compact(value)}` : `$${value.toFixed(2)}`,
        unit: 'MTok',
      };
    }
    const magnitude = Math.abs(value);
    if (magnitude >= 1_000_000_000)
      return { primary: compact(value / 1_000_000_000), unit: 'billion' };
    if (magnitude >= 1_000_000) return { primary: compact(value / 1_000_000), unit: 'million' };
    if (magnitude >= 1_000) return { primary: compact(value / 1_000), unit: 'thousand' };
    return { primary: compact(value), unit: 'tokens' };
  }
  function metricTitle() {
    if (settings.totalSpendMetric === 'tokens') return 'Tokens';
    if (settings.totalSpendMetric === 'costPerMillion') return 'Cost/MTok';
    return 'Cost';
  }
  function patch(patch: Partial<AppSettings>) {
    onChange({ ...settings, ...patch });
  }
  async function share() {
    if (!(await onShare(projection))) return;
    shareCopied = true;
    if (shareTimer) clearTimeout(shareTimer);
    shareTimer = setTimeout(() => (shareCopied = false), 1400);
  }
</script>

<section class="total-spend-section" aria-label="Total Spend" data-total-spend>
  <div class="total-card__header">
    <div class="total-card__title">
      <SelectMenu
        label="Total Spend Metric"
        value={settings.totalSpendMetric}
        variant="title"
        options={[
          { value: 'cost', label: 'Cost' },
          { value: 'costPerMillion', label: 'Cost/MTok' },
          { value: 'tokens', label: 'Tokens' },
        ]}
        onChange={(value) => patch({ totalSpendMetric: value as AppSettings['totalSpendMetric'] })}
      />
      <span
        class="icon-button icon-button--plain total-card__info"
        data-tooltip={`Only includes ${providerNames.join(' and ')}.`}
        aria-label={`Only includes ${providerNames.join(' and ')}`}
        role="img"><Icon name="about" size={13} strokeWidth={1.9} /></span
      >
    </div>
    <button
      class="icon-button icon-button--plain total-card__share"
      type="button"
      aria-label={`Share ${metricTitle()} Screenshot`}
      data-tooltip="Share Screenshot"
      onclick={share}
      ><Icon name={shareCopied ? 'check' : 'share'} size={14} strokeWidth={1.8} /></button
    >
  </div>
  <div class="total-card">
    <div class="period-switcher" aria-label="Total Spend period">
      <span
        class="period-switcher__selection"
        style={`transform: translateX(${periodIndex * 100}%)`}
        aria-hidden="true"
      ></span>
      {#each [['today', 'Today'], ['yesterday', 'Yesterday'], ['last30Days', '30 Days']] as option (option[0])}
        <button
          class:active={settings.totalSpendPeriod === option[0]}
          type="button"
          onclick={() => patch({ totalSpendPeriod: option[0] as AppSettings['totalSpendPeriod'] })}
          >{option[1]}</button
        >
      {/each}
    </div>
    {#if projection.centerValue === null}
      <div class="total-card__empty">
        <span>{emptySpendMessage(settings.totalSpendMetric)}</span>
      </div>
    {:else}
      <div class="total-card__body">
        <div class="spend-ring">
          <svg viewBox="0 0 104 104" aria-hidden="true">
            <circle class="spend-ring__track" cx="52" cy="52" r="40" pathLength="100" />
            {#each ringSegments as segment (segment.id)}
              <circle
                class="spend-ring__segment"
                cx="52"
                cy="52"
                r="40"
                pathLength="100"
                style={`--segment-color: var(--provider-${segment.id}, var(--provider)); --segment-length: ${segment.length}; --segment-offset: ${segment.offset}`}
              />
            {/each}
          </svg>
          <div class="spend-ring__label">
            <strong>{ringCenter(projection.centerValue).primary}</strong><span
              >{ringCenter(projection.centerValue).unit}</span
            >
          </div>
        </div>
        <div class="spend-legend">
          {#each projection.slices as provider (provider.id)}
            <span
              ><i style={`background: var(--provider-${provider.id}, var(--provider))`}
              ></i>{providerDisplayName(provider.id)}</span
            ><strong>{display(provider.value)}</strong>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</section>

<style>
  :global {
    .total-card {
      margin-bottom: 12px;
      padding: 10px 11px;
      border-radius: 12px;
      background: var(--card);
    }

    .total-card__header,
    .total-card__body,
    .trend-heading {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
    }

    .period-switcher {
      position: relative;
      display: flex;
      padding: 2px;
      border-radius: 7px;
      background: var(--meter-track);
    }

    .period-switcher button {
      position: relative;
      z-index: 1;
      padding: 3px 6px;
      border: 0;
      border-radius: 5px;
      color: var(--secondary);
      background: transparent;
      font-size: 9px;
      cursor: pointer;
    }

    .period-switcher button.active {
      color: var(--text);
      background: transparent;
      box-shadow: none;
    }

    .period-switcher__selection {
      position: absolute;
      top: 3px;
      bottom: 3px;
      left: 3px;
      width: calc((100% - 6px) / 3);
      border-radius: 999px;
      background: var(--tray);
      box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
      transition: transform var(--motion-spring);
    }

    .total-card__body {
      justify-content: flex-start;
      padding-top: 10px;
    }

    .spend-ring {
      position: relative;
      display: grid;
      width: 74px;
      height: 74px;
      flex: 0 0 74px;
      place-items: center;
    }

    .spend-ring svg {
      position: absolute;
      width: 100%;
      height: 100%;
      overflow: visible;
      transform: rotate(-90deg);
    }

    .spend-ring circle {
      fill: none;
      stroke-width: 12;
    }

    .spend-ring__track {
      stroke: var(--meter-track);
    }

    .spend-ring__segment {
      stroke: var(--segment-color);
      stroke-dasharray: var(--segment-length) calc(100 - var(--segment-length));
      stroke-dashoffset: calc(-1 * var(--segment-offset));
      stroke-linecap: round;
      transition:
        stroke-dasharray var(--motion-spring),
        stroke-dashoffset var(--motion-spring);
    }

    .spend-ring__label {
      position: relative;
      z-index: 1;
      display: flex;
      align-items: center;
      justify-content: center;
      flex-direction: column;
    }

    .spend-ring strong {
      max-width: 48px;
      overflow: hidden;
      font-size: 13px;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .spend-ring span,
    .spend-legend small {
      color: var(--secondary);
      font-size: 8px;
    }

    .spend-legend {
      display: grid;
      flex: 1;
      grid-template-columns: 1fr auto;
      gap: 2px 8px;
      font-size: 11px;
    }

    .spend-legend span i {
      display: inline-block;
      width: 7px;
      height: 7px;
      margin-right: 5px;
      border-radius: 50%;
      background: var(--provider);
    }

    .spend-legend small {
      grid-column: 1 / -1;
    }

    .icon-button {
      display: grid;
      width: 30px;
      height: 30px;
      margin-left: 0;
      padding: 0;
      border: 0;
      border-radius: 50%;
      color: var(--secondary);
      background: transparent;
      cursor: default;
      place-items: center;
    }

    .icon-button:hover,
    .icon-button:focus-visible {
      color: var(--text);
      background: var(--button-hover);
    }

    .icon-button:focus-visible {
      outline: 2px solid var(--meter-fill);
      outline-offset: 2px;
    }

    @media (prefers-reduced-motion: no-preference) {
      .icon-button {
        transition:
          color 120ms ease,
          background-color 120ms ease;
      }
    }

    .total-spend-section {
      margin-bottom: 0;
    }

    .total-card__header {
      min-height: 24px;
      margin-bottom: 4px;
      padding: 0 4px 2px;
    }

    .total-card__title {
      display: flex;
      min-width: 0;
      align-items: center;
      gap: 4px;
    }

    .select-menu--title {
      min-width: 0;
    }

    .select-menu--title .select-menu__trigger {
      min-height: 24px;
      gap: 4px;
      padding: 0;
      border: 0;
      background: transparent;
      font-size: 14px;
      font-weight: 600;
    }

    .select-menu--title .select-menu__trigger:hover,
    .select-menu--title .select-menu__trigger[aria-expanded='true'] {
      color: var(--meter-fill);
      background: transparent;
    }

    .select-menu__list--title {
      min-width: 138px;
      transform-origin: top left;
    }

    .select-menu__list--title.select-menu__list--above {
      transform-origin: bottom left;
    }

    .total-card__header .icon-button--plain {
      width: 20px;
      height: 20px;
      flex: 0 0 20px;
      color: var(--secondary);
      cursor: pointer;
    }

    .total-card__header .total-card__info {
      width: 16px;
      height: 20px;
      flex-basis: 16px;
      cursor: help;
    }

    .total-card {
      margin: 0;
      padding: 12px 14px;
    }

    .period-switcher {
      width: 100%;
      padding: 3px;
      background: var(--meter-track);
    }

    .period-switcher button {
      min-height: 23px;
      flex: 1;
      padding: 4px 12px;
      font-size: 11px;
      font-weight: 500;
    }

    .period-switcher button.active {
      font-weight: 600;
    }

    .total-card__body {
      gap: 18px;
      padding-top: 12px;
    }

    .spend-ring {
      width: 104px;
      height: 104px;
      flex-basis: 104px;
      padding: 0;
    }

    .spend-ring strong {
      max-width: 68px;
    }

    .spend-legend {
      align-content: center;
      gap: 7px 8px;
    }

    .spend-legend strong {
      color: var(--secondary);
      font-weight: 500;
      text-align: right;
    }

    .total-card__empty {
      display: flex;
      min-height: 76px;
      align-items: center;
      justify-content: center;
      flex-direction: column;
      gap: 8px;
      color: var(--secondary);
      font-size: 11px;
      text-align: center;
    }

    :root[data-density='compact'] .total-card {
      padding: 10px 12px;
    }

    :root[data-density='compact'] .period-switcher button {
      min-height: 21px;
      padding: 3px 10px;
    }

    :root[data-density='compact'] .total-card__body {
      gap: 14px;
      padding-top: 8px;
    }

    :root[data-density='compact'] .spend-ring {
      width: 88px;
      height: 88px;
      flex-basis: 88px;
    }

    :root[data-density='compact'] .spend-ring strong {
      max-width: 58px;
    }

    :root[data-density='compact'] .total-card__empty {
      min-height: 64px;
    }
  }
</style>
