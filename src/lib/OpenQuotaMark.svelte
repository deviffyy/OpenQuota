<script lang="ts">
  interface Props {
    size?: number;
    decorative?: boolean;
  }

  let { size = 18, decorative = true }: Props = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  xmlns="http://www.w3.org/2000/svg"
  aria-hidden={decorative}
  aria-label={decorative ? undefined : 'OpenQuota'}
  role={decorative ? undefined : 'img'}
>
  <circle cx="12" cy="12" r="8" stroke="currentColor" stroke-width="2.5" />
  <path d="M15.2 15.2L20 20" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" />
  <path d="M7.5 12H12.3" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" />
</svg>
