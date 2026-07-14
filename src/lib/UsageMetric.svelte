<script lang="ts">
  import { onDestroy } from 'svelte';
  import Icon from './Icon.svelte';
  import ModelUsageDetail from './ModelUsageDetail.svelte';
  import type { UsagePeriod } from './types';

  interface Props {
    label: string;
    period: UsagePeriod | null;
  }
  let { label, period }: Props = $props();
  let open = $state(false);
  let detailTop = $state(8);
  let showTimer: ReturnType<typeof setTimeout> | undefined;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  function compact(value: number) {
    return new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(
      value,
    );
  }
  function reading(value: UsagePeriod | null) {
    if (!value) return 'No data';
    const tokens = `${compact(value.tokens)} tokens`;
    if (value.estimatedCostUsd === null) return tokens;
    return `${value.estimateComplete ? '' : '~'}$${value.estimatedCostUsd.toFixed(2)} · ${tokens}`;
  }
  function unknownModelTooltip(models: string[]) {
    const heading = models.length === 1 ? 'Unknown model found' : 'Unknown models found';
    return [heading, ...models.map((model) => `- ${model}`)].join('\n');
  }
  function scheduleShow(event: Event) {
    if (!period?.modelBreakdown?.models.length || open || showTimer) return;
    if (hideTimer) clearTimeout(hideTimer);
    const target = event.currentTarget as HTMLElement;
    const rect = target.getBoundingClientRect();
    const estimatedHeight = Math.min(86 + period.modelBreakdown.models.length * 52, 360);
    detailTop = Math.max(8, Math.min(rect.bottom + 7, window.innerHeight - estimatedHeight - 8));
    showTimer = setTimeout(() => {
      open = true;
      showTimer = undefined;
    }, 400);
  }
  function scheduleHide() {
    if (showTimer) clearTimeout(showTimer);
    showTimer = undefined;
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      open = false;
      hideTimer = undefined;
    }, 180);
  }
  function keepOpen() {
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = undefined;
  }
  onDestroy(() => {
    if (showTimer) clearTimeout(showTimer);
    if (hideTimer) clearTimeout(hideTimer);
  });
</script>

<div class="usage-row">
  <span
    >{label}{#if period?.unknownModels?.length}<i
        class="usage-label-warning"
        data-tooltip={unknownModelTooltip(period.unknownModels)}
        aria-label="This period used a model with unknown pricing"
        ><Icon name="warning" size={10} strokeWidth={2.2} /></i
      >{/if}</span
  >
  <button
    type="button"
    class:usage-reading-interactive={period?.modelBreakdown?.models.length}
    disabled={!period?.modelBreakdown?.models.length}
    onmouseenter={scheduleShow}
    onmouseleave={scheduleHide}
    onfocus={scheduleShow}
    onblur={scheduleHide}>{reading(period)}</button
  >
</div>

{#if open && period?.modelBreakdown}
  <ModelUsageDetail
    title={label}
    breakdown={period.modelBreakdown}
    top={detailTop}
    onEnter={keepOpen}
    onLeave={scheduleHide}
  />
{/if}

<style>
  :global {
    .usage-row {
      padding: 6px 14px;
      font-size: 12px;
      line-height: 15px;
    }

    .usage-row--condensed {
      padding-top: 2px;
    }

    .usage-row span {
      color: color-mix(in srgb, var(--text) 88%, var(--card));
      font-weight: 600;
    }

    .usage-row strong,
    .usage-row > button {
      font-weight: 450;
      text-align: right;
    }

    .usage-row > button {
      padding: 0;
      border: 0;
      color: inherit;
      background: none;
      font: inherit;
    }

    .usage-row > button:disabled {
      opacity: 1;
    }

    .usage-reading-interactive {
      padding: 0 1px;
      border-radius: 6px;
      outline: none;
      cursor: default;
      transition:
        background-color 120ms ease,
        box-shadow 120ms ease;
    }

    .usage-reading-interactive:hover,
    .usage-reading-interactive:focus-visible {
      background: var(--button-hover);
      box-shadow: 0 0 0 4px var(--button-hover);
    }

    .usage-label-warning {
      display: inline-block;
      margin-left: 4px;
      color: var(--meter-warning);
      font-style: normal;
      font-size: 9px;
      transform: translateY(-1px);
    }

    .usage-divider {
      height: 1px;
      margin: 3px 0 5px;
      background: var(--separator);
    }

    :root[data-density='compact'] .usage-row {
      padding: 3px 14px;
      font-size: 11px;
    }
  }
</style>
