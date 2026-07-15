<script lang="ts">
  import { metricDefinition } from './metrics';
  import QuotaMetric from './QuotaMetric.svelte';
  import UsageMetric from './UsageMetric.svelte';
  import UsageTrend from './UsageTrend.svelte';
  import ValueMetric from './ValueMetric.svelte';
  import type { AppSettings, MetricLayout, ProviderSnapshot } from './types';

  interface Props {
    layout: MetricLayout;
    snapshot: ProviderSnapshot;
    settings: AppSettings;
    now: number;
    onSettingsChange: (settings: AppSettings) => void;
  }
  let { layout, snapshot, settings, now, onSettingsChange }: Props = $props();
  const definition = $derived(metricDefinition(layout.id));
  const quota = $derived(
    definition?.kind === 'quota' || definition?.kind === 'quotaOrValue'
      ? snapshot.quotas.find((item) => item.id === definition.sourceId)
      : undefined,
  );
  const isSessionWindow = $derived(
    (definition?.kind === 'quota' || definition?.kind === 'quotaOrValue') &&
      ((snapshot.providerId === 'claude' && definition.sourceId === 'session') ||
        (snapshot.providerId === 'antigravity' &&
          (definition.sourceId === 'geminiPro' || definition.sourceId === 'claude'))),
  );
  const period = $derived.by(() => {
    if (definition?.kind !== 'usage') return null;
    if (definition.sourceId === 'today') return snapshot.usage.today;
    if (definition.sourceId === 'yesterday') return snapshot.usage.yesterday;
    return snapshot.usage.last30Days;
  });
  const valueMetric = $derived(
    definition?.kind === 'value' || definition?.kind === 'quotaOrValue'
      ? (snapshot.valueMetrics.find((item) => item.id === definition.sourceId) ?? null)
      : null,
  );
  const usageSourceNote = $derived(
    snapshot.providerId === 'claude'
      ? 'From your Claude usage history (estimated)'
      : snapshot.providerId === 'codex'
        ? 'From your Codex logs (estimated)'
        : `From your ${snapshot.providerId} usage history`,
  );
</script>

{#if (definition?.kind === 'quota' || definition?.kind === 'quotaOrValue') && quota}
  <QuotaMetric
    {quota}
    {now}
    usageDisplay={settings.usageDisplay}
    resetDisplay={settings.resetDisplay}
    timeFormat={settings.timeFormat}
    alwaysShowPacing={settings.alwaysShowPacing}
    {isSessionWindow}
    onToggleUsage={() =>
      onSettingsChange({
        ...settings,
        usageDisplay: settings.usageDisplay === 'used' ? 'left' : 'used',
      })}
    onToggleReset={() =>
      onSettingsChange({
        ...settings,
        resetDisplay: settings.resetDisplay === 'countdown' ? 'exact' : 'countdown',
      })}
  />
{:else if definition?.kind === 'quotaOrValue' && valueMetric}
  <ValueMetric
    label={definition.label}
    metric={valueMetric}
    {now}
    resetDisplay={settings.resetDisplay}
    timeFormat={settings.timeFormat}
  />
{:else if definition?.kind === 'quota' || definition?.kind === 'quotaOrValue'}
  <section class="metric metric--no-data" aria-label={`${definition.label} quota`}>
    <div class="metric__heading"><h2>{definition.label}</h2></div>
    <div class="meter-shell">
      <div
        class="meter"
        role="progressbar"
        aria-label={`${definition.label} used`}
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow="0"
      ></div>
    </div>
    <div class="metric__reading"><span>No data</span><span>Reset unavailable</span></div>
  </section>
{:else if definition?.kind === 'trend'}
  <UsageTrend daily={snapshot.usage.daily} sourceNote={usageSourceNote} />
{:else if definition?.kind === 'usage'}
  <UsageMetric label={definition.label} {period} />
{:else if definition?.kind === 'value'}
  <ValueMetric
    label={definition.label}
    metric={valueMetric}
    {now}
    resetDisplay={settings.resetDisplay}
    timeFormat={settings.timeFormat}
  />
{/if}
