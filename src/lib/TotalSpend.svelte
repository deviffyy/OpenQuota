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
