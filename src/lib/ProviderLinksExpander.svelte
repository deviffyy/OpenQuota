<script lang="ts">
  import { slide } from 'svelte/transition';
  import Icon from './Icon.svelte';
  import { springMotion } from './motion';
  import ProviderLinks from './ProviderLinks.svelte';
  import type { ProviderLink } from './types';

  interface Props {
    providerId: string;
    links: ProviderLink[];
    expanded: boolean;
    reducedMotion: boolean;
    onToggle: () => void;
    onOpen: (linkIndex: number) => void;
  }

  let { providerId, links, expanded, reducedMotion, onToggle, onOpen }: Props = $props();
</script>

{#if links.length > 0}
  <button
    class="demand-divider"
    data-reorder-group={`dashboard-metrics:${providerId}`}
    data-reorder-id="section:onDemand"
    type="button"
    aria-expanded={expanded}
    aria-label={expanded ? 'Show less' : 'Show more'}
    onclick={onToggle}
  >
    <Icon name={expanded ? 'chevron-up' : 'chevron-down'} size={10} strokeWidth={2.2} />
  </button>
  {#if expanded}
    <div class="demand-metrics" transition:slide={springMotion(reducedMotion)}>
      <ProviderLinks {links} {onOpen} />
    </div>
  {/if}
{/if}
