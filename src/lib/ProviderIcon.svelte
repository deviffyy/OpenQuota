<script lang="ts">
  import { providerIconColor, providerIconPath } from './providerIconPaths';

  interface Props {
    providerId: string;
    size?: number;
  }

  let { providerId, size = 18 }: Props = $props();
  const path = $derived(providerIconPath(providerId));
  const color = $derived(providerIconColor(providerId));
</script>

<svg
  class="provider-icon"
  width={size}
  height={size}
  viewBox="0 0 100 100"
  fill="none"
  aria-hidden="true"
>
  <path d={path} fill={color ?? 'currentColor'} />
</svg>

<style>
  .provider-icon {
    display: block;
    flex: 0 0 auto;
  }
</style>
