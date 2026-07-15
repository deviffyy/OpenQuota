<script lang="ts">
  import Icon from './Icon.svelte';
  import type { ProviderNotice } from './types';

  interface Props {
    notice: ProviderNotice;
  }

  let { notice }: Props = $props();
</script>

<div
  class="provider-notice-row provider-notice-row--{notice.tone}"
  role={notice.tone === 'warning' ? 'status' : undefined}
>
  <span class="provider-notice-icon"><Icon name="warning" size={12} strokeWidth={2} /></span>
  <span class="provider-notice-copy"
    ><strong>{notice.title}</strong><small>{notice.message}</small></span
  >
</div>

<style>
  :global {
    .provider-notice-row {
      display: flex;
      align-items: center;
      gap: 9px;
      margin: 6px 8px;
      padding: 8px 9px;
      border: 1px solid color-mix(in srgb, var(--warning) 18%, transparent);
      border-radius: 9px;
      color: var(--warning);
      background: color-mix(in srgb, var(--warning-bg) 72%, transparent);
    }

    .provider-notice-icon {
      display: grid;
      width: 24px;
      height: 24px;
      flex: 0 0 24px;
      border-radius: 7px;
      background: color-mix(in srgb, var(--warning) 12%, transparent);
      place-items: center;
    }

    .provider-notice-copy {
      display: grid;
      min-width: 0;
      gap: 1px;
    }

    .provider-notice-copy strong {
      color: var(--text);
      font-size: 11px;
      font-weight: 650;
      line-height: 14px;
    }

    .provider-notice-copy small {
      color: var(--secondary);
      font-size: 10px;
      line-height: 13px;
      overflow-wrap: anywhere;
    }

    :root[data-density='compact'] .provider-notice-row {
      margin: 4px 8px;
      padding: 5px 8px;
    }

    :root[data-density='compact'] .provider-notice-icon {
      width: 20px;
      height: 20px;
      flex-basis: 20px;
    }
  }
</style>
