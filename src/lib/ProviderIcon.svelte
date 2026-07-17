<script lang="ts">
  import { providerIconColor, providerIconPath, providerIconViewBox } from './providerIconPaths';

  interface Props {
    providerId: string;
    size?: number;
  }

  let { providerId, size = 18 }: Props = $props();
  const path = $derived(providerIconPath(providerId));
  const color = $derived(providerIconColor(providerId));
  const viewBox = $derived(providerIconViewBox(providerId));
</script>

<svg class="provider-icon" width={size} height={size} {viewBox} fill="none" aria-hidden="true">
  <path d={path} fill={color ?? 'currentColor'} />
</svg>

<style>
  .provider-icon {
    display: block;
    flex: 0 0 auto;
  }
</style>
