<script lang="ts">
  import Icon from './Icon.svelte';
  import type { ProviderLink } from './types';

  interface Props {
    links: ProviderLink[];
    onOpen: (linkIndex: number) => void;
  }

  let { links, onOpen }: Props = $props();
  const columns = $derived(Math.min(3, Math.max(1, links.length)));
</script>

<div class="provider-links" style={`--provider-link-columns: ${columns}`}>
  {#each links as link, linkIndex (`${link.label}:${link.url}`)}
    <button
      type="button"
      aria-label={`${link.label}, opens in browser`}
      onclick={() => onOpen(linkIndex)}
    >
      <span>{link.label}</span><Icon name="external-link" size={10} strokeWidth={1.8} />
    </button>
  {/each}
</div>

<style>
  .provider-links {
    display: grid;
    grid-template-columns: repeat(var(--provider-link-columns), minmax(0, 1fr));
    gap: 6px;
    padding: 7px 6px;
  }

  button {
    display: flex;
    min-width: 0;
    min-height: 26px;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 4px 8px;
    border: 1px solid var(--separator);
    border-radius: 6px;
    color: var(--text);
    background: transparent;
    font: inherit;
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
  }

  button:hover,
  button:focus-visible {
    outline: none;
    background: var(--button-hover);
  }

  button:focus-visible {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--provider) 35%, transparent);
  }

  span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  :global(html[data-density='compact']) .provider-links {
    gap: 5px;
    padding-block: 5px;
  }

  :global(html[data-density='compact']) button {
    min-height: 24px;
    font-size: 9px;
  }
</style>
