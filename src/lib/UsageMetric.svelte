<script lang="ts">
  import type { UsagePeriod } from './types';

  interface Props {
    label: string;
    period: UsagePeriod | null;
  }
  let { label, period }: Props = $props();

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
</script>

<div class="usage-row">
  <span>{label}</span><strong>{reading(period)}</strong>
</div>
