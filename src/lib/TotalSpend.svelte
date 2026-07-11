<script lang="ts">
  import Icon from './Icon.svelte';
  import { providerDisplayName } from './metrics';
  import { emptySpendMessage, projectSpend, type SpendProjection } from './totalSpend';
  import type { AppSettings, UsageHistory } from './types';

  interface Props {
    providers: Array<{ id: string; usage: UsageHistory }>;
    settings: AppSettings;
    onChange: (settings: AppSettings) => void;
    onShare: (projection: SpendProjection) => void;
  }
  let { providers, settings, onChange, onShare }: Props = $props();
  const projection = $derived(
    projectSpend(providers, settings.totalSpendPeriod, settings.totalSpendMetric),
  );
  const tokenProjection = $derived(projectSpend(providers, settings.totalSpendPeriod, 'tokens'));
  const providerNames = $derived(providers.map((provider) => providerDisplayName(provider.id)));
  const ringGradient = $derived.by(() => {
    const total = projection.slices.reduce((sum, provider) => sum + provider.value, 0);
    if (total <= 0) return 'var(--meter-track)';
    if (projection.slices.length === 1) {
      return `var(--provider-${projection.slices[0].id}, var(--provider))`;
    }
    const floored = projection.slices.map((provider) => Math.max(provider.value / total, 0.025));
    const flooredTotal = floored.reduce((sum, share) => sum + share, 0);
    let cursor = 0;
    const stops = projection.slices.flatMap((provider, index) => {
      const start = cursor;
      cursor += (floored[index] / flooredTotal) * 100;
      const gap = Math.min(0.8, (cursor - start) * 0.15);
      return [
        `var(--card) ${start}% ${start + gap / 2}%`,
        `var(--provider-${provider.id}, var(--provider)) ${start + gap / 2}% ${cursor - gap / 2}%`,
        `var(--card) ${cursor - gap / 2}% ${cursor}%`,
      ];
    });
    return `conic-gradient(${stops.join(', ')})`;
  });

  function display(value: number | null) {
    if (value === null) return '—';
    if (settings.totalSpendMetric === 'tokens') {
      return new Intl.NumberFormat('en-US', {
        notation: 'compact',
        maximumFractionDigits: 1,
      }).format(value);
    }
    return `$${value >= 100 ? value.toFixed(0) : value.toFixed(2)}`;
  }
  function unit(value: number | null) {
    if (value === null) return '';
    if (settings.totalSpendMetric === 'tokens') return 'tokens';
    return settings.totalSpendMetric === 'costPerMillion' ? '/ MTok' : 'estimated';
  }
  function metricTitle() {
    if (settings.totalSpendMetric === 'tokens') return 'Tokens';
    if (settings.totalSpendMetric === 'costPerMillion') return 'Cost/MTok';
    return 'Cost';
  }
  function patch(patch: Partial<AppSettings>) {
    onChange({ ...settings, ...patch });
  }
</script>

<section class="total-spend-section" aria-label="Total Spend" data-total-spend>
  <div class="total-card__header">
    <div class="total-card__title">
      <select
        aria-label="Total Spend metric"
        value={settings.totalSpendMetric}
        onchange={(event) =>
          patch({ totalSpendMetric: event.currentTarget.value as AppSettings['totalSpendMetric'] })}
      >
        <option value="cost">Cost</option>
        <option value="costPerMillion">Cost/MTok</option>
        <option value="tokens">Tokens</option>
      </select>
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
      onclick={() => onShare(projection)}><Icon name="share" size={14} strokeWidth={1.8} /></button
    >
  </div>
  <div class="total-card">
    <div class="period-switcher" aria-label="Total Spend period">
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
        {#if settings.totalSpendMetric !== 'tokens' && tokenProjection.centerValue !== null}
          <button type="button" onclick={() => patch({ totalSpendMetric: 'tokens' })}
            >View available tokens</button
          >
        {/if}
      </div>
    {:else}
      <div class="total-card__body">
        <div class="spend-ring" style={`background: ${ringGradient}`}>
          <div>
            <strong>{display(projection.centerValue)}</strong><span
              >{unit(projection.centerValue)}</span
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
