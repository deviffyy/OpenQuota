<script lang="ts">
  import type { ModelUsageBreakdown } from './types';

  interface Props {
    title: string;
    breakdown: ModelUsageBreakdown;
    top: number;
    onEnter: () => void;
    onLeave: () => void;
  }

  let { title, breakdown, top, onEnter, onLeave }: Props = $props();

  const shares = $derived.by(() => {
    const allPriced = breakdown.models.every((model) => model.costUsd !== null);
    const costTotal = breakdown.models.reduce((sum, model) => sum + (model.costUsd ?? 0), 0);
    const tokenTotal = breakdown.models.reduce((sum, model) => sum + model.totalTokens, 0);
    if (allPriced && costTotal > 0) {
      return breakdown.models.map((model) => Math.max(0, (model.costUsd ?? 0) / costTotal));
    }
    return breakdown.models.map((model) =>
      tokenTotal > 0 ? Math.max(0, model.totalTokens / tokenTotal) : 0,
    );
  });

  const percents = $derived.by(() => {
    if (!shares.some((share) => share > 0)) return shares.map(() => 0);
    const raw = shares.map((share) => share * 100);
    const result = raw.map(Math.floor);
    let remaining = 100 - result.reduce((sum, value) => sum + value, 0);
    const order = raw
      .map((value, index) => ({ index, remainder: value - Math.floor(value) }))
      .sort((left, right) => right.remainder - left.remainder || left.index - right.index);
    for (const item of order) {
      if (remaining <= 0) break;
      result[item.index] += 1;
      remaining -= 1;
    }
    return result;
  });

  function compact(value: number) {
    return new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(
      value,
    );
  }
</script>

<div
  class="model-usage-detail"
  style={`top:${top}px`}
  role="tooltip"
  aria-label={`${title} model usage`}
  onmouseenter={onEnter}
  onmouseleave={onLeave}
>
  <h3>{title}</h3>
  <div class="model-usage-list">
    {#each breakdown.models as model, index (model.model)}
      <div class="model-usage-row">
        <div class="model-usage-primary">
          <strong title={model.model}>{model.model}</strong>
          <span>{model.costUsd === null ? '—' : `$${model.costUsd.toFixed(2)}`}</span>
        </div>
        <div class="model-usage-secondary">
          <span>{percents[index]}%</span><span>{compact(model.totalTokens)} tokens</span>
        </div>
        <div class="model-usage-meter" aria-hidden="true">
          <i style={`width:${Math.min(shares[index] * 100, 100)}%`}></i>
        </div>
      </div>
    {/each}
  </div>
  <p>{breakdown.sourceNote}</p>
</div>
