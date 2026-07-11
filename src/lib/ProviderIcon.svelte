<script lang="ts">
  import { providerIconPath } from './providerIconPaths';

  interface Props {
    providerId: string;
    size?: number;
  }

  let { providerId, size = 18 }: Props = $props();
  const path = $derived(providerIconPath(providerId));
</script>

<svg
  class="provider-icon"
  width={size}
  height={size}
  viewBox="0 0 100 100"
  fill="none"
  aria-hidden="true"
>
  <path
    d={path}
    fill={providerId === 'claude'
      ? '#DE7356'
      : providerId === 'antigravity'
        ? '#4285F4'
        : 'currentColor'}
  />
</svg>

<style>
  .provider-icon {
    display: block;
    flex: 0 0 auto;
  }
</style>
