<script lang="ts">
  import Icon from './Icon.svelte';
  import { formatLimit, formatReset, projectPace } from './pacing';
  import type { QuotaWindow } from './types';

  interface Props {
    quota: QuotaWindow;
    now: number;
    usageDisplay: 'used' | 'left';
    resetDisplay: 'countdown' | 'exact';
    timeFormat: 'system' | 'twelveHour' | 'twentyFourHour';
    alwaysShowPacing: boolean;
    onToggleUsage: () => void;
    onToggleReset: () => void;
  }

  let {
    quota,
    now,
    usageDisplay,
    resetDisplay,
    timeFormat,
    alwaysShowPacing,
    onToggleUsage,
    onToggleReset,
  }: Props = $props();
  const used = $derived(Math.min(100, Math.max(0, quota.usedPercent)));
  const remaining = $derived(Math.max(0, 100 - used));
  const reading = $derived.by(() => {
    if (quota.format === 'dollars' && quota.usedValue !== null) {
      if (usageDisplay === 'left' && quota.limitValue !== null) {
        return `$${Math.max(0, quota.limitValue - quota.usedValue).toFixed(2)} left`;
      }
      return `$${quota.usedValue.toFixed(2)} spent`;
    }
    return `${(usageDisplay === 'used' ? used : remaining).toFixed(0)}% ${usageDisplay}`;
  });
  const pace = $derived(projectPace(quota, now));
  const severity = $derived(
    pace.severity === 'level'
      ? used >= 90
        ? 'critical'
        : used >= 80
          ? 'warning'
          : 'normal'
      : pace.severity === 'healthy'
        ? 'normal'
        : pace.severity === 'close'
          ? 'warning'
          : 'critical',
  );
  const showPace = $derived(
    pace.severity === 'close' ||
      pace.severity === 'runningOut' ||
      pace.severity === 'spent' ||
      (alwaysShowPacing && pace.severity === 'healthy'),
  );
  const paceLabel = $derived.by(() => {
    if (pace.severity === 'spent') return 'Limit reached';
    if (pace.severity === 'runningOut')
      return formatLimit(pace.runOutAt, now, resetDisplay, timeFormat);
    if (pace.projectedUsedPercent === null) return null;
    const left = Math.max(0, 100 - pace.projectedUsedPercent);
    return pace.severity === 'close'
      ? `~${Math.max(1, Math.round(left))}% spare`
      : `~${Math.round(left)}% left at reset`;
  });
</script>

<section class="metric" aria-label={`${quota.label} quota`}>
  <div class="metric__heading">
    <h2>{quota.label}</h2>
    {#if showPace && paceLabel}
      <span class:pace-critical={severity === 'critical'}
        >{#if pace.severity === 'runningOut'}<Icon
            name="warning"
            size={11}
            strokeWidth={2}
          />{/if}{paceLabel}</span
      >
    {/if}
  </div>

  <div
    class="meter meter--{severity}"
    role="progressbar"
    aria-label={`${quota.label} used`}
    aria-valuemin="0"
    aria-valuemax="100"
    aria-valuenow={used}
  >
    <span class="meter__fill" style={`width: ${used}%`}></span>
    {#if showPace && pace.evenPacePercent !== null}
      <span
        class="meter__pace"
        style={`left: ${Math.min(100, Math.max(0, pace.evenPacePercent))}%`}
        aria-hidden="true"
      ></span>
    {/if}
  </div>

  <div class="metric__reading">
    <button type="button" onclick={onToggleUsage}>
      {reading}
    </button>
    <button type="button" onclick={onToggleReset}>
      {formatReset(quota.resetsAt, now, resetDisplay, timeFormat)}
    </button>
  </div>
</section>
