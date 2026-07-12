<script lang="ts">
  import { metricDefinition } from './metrics';
  import QuotaMetric from './QuotaMetric.svelte';
  import UsageMetric from './UsageMetric.svelte';
  import UsageTrend from './UsageTrend.svelte';
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
    definition?.kind === 'quota'
      ? snapshot.quotas.find((item) => item.id === definition.sourceId)
      : undefined,
  );
  const period = $derived.by(() => {
    if (definition?.kind !== 'usage') return null;
    if (definition.sourceId === 'today') return snapshot.usage.today;
    if (definition.sourceId === 'yesterday') return snapshot.usage.yesterday;
    return snapshot.usage.last30Days;
  });
  const usageSourceNote = $derived(
    snapshot.providerId === 'claude'
      ? 'From your Claude usage history (estimated)'
      : snapshot.providerId === 'codex'
        ? 'From your Codex logs (estimated)'
        : `From your ${snapshot.providerId} usage history`,
  );
</script>

{#if definition?.kind === 'quota' && quota}
  <QuotaMetric
    {quota}
    {now}
    usageDisplay={settings.usageDisplay}
    resetDisplay={settings.resetDisplay}
    timeFormat={settings.timeFormat}
    alwaysShowPacing={settings.alwaysShowPacing}
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
{:else if definition?.kind === 'trend'}
  <UsageTrend daily={snapshot.usage.daily} sourceNote={usageSourceNote} />
{:else if definition?.kind === 'usage'}
  <UsageMetric label={definition.label} {period} />
{/if}
