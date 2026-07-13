<script lang="ts">
  type IconName =
    | 'about'
    | 'back'
    | 'check'
    | 'close'
    | 'chevron-down'
    | 'chevron-left'
    | 'chevron-up'
    | 'chevron-right'
    | 'gear'
    | 'flame-filled'
    | 'grip-dots'
    | 'grip-lines'
    | 'power'
    | 'refresh'
    | 'reset'
    | 'share'
    | 'sliders'
    | 'star'
    | 'star-filled'
    | 'warning';

  interface Props {
    name: IconName;
    size?: number;
    strokeWidth?: number;
  }

  let { name, size = 16, strokeWidth = 1.8 }: Props = $props();
</script>

<svg
  class="symbol-icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === 'back'}
    <path d="m14.5 5-7 7 7 7" />
  {:else if name === 'chevron-left'}
    <path d="m14.5 5-7 7 7 7" />
  {:else if name === 'chevron-right'}
    <path d="m9.5 5 7 7-7 7" />
  {:else if name === 'chevron-down'}
    <path d="m6 9 6 6 6-6" />
  {:else if name === 'chevron-up'}
    <path d="m6 15 6-6 6 6" />
  {:else if name === 'reset'}
    <path d="M4 4v6h6" /><path d="M5.6 16.5a8 8 0 1 0 .6-9.5L4 10" />
  {:else if name === 'refresh'}
    <path d="M20 7v5h-5" /><path d="M4 17v-5h5" /><path
      d="M6.1 8a7 7 0 0 1 11.4-1L20 12M4 12l2.5 5a7 7 0 0 0 11.4-1"
    />
  {:else if name === 'sliders'}
    <path d="M4 7h5M15 7h5M4 17h9M17 17h3" /><circle cx="12" cy="7" r="3" /><circle
      cx="15"
      cy="17"
      r="2"
    />
  {:else if name === 'gear'}
    <circle cx="12" cy="12" r="3" /><path
      d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1a1.7 1.7 0 0 0 1.9.3A1.7 1.7 0 0 0 10 3V2.8h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z"
    />
  {:else if name === 'share'}
    <path d="M12 3v12M8 7l4-4 4 4" /><path d="M6 11H4v9h16v-9h-2" />
  {:else if name === 'about'}
    <circle cx="12" cy="12" r="9" /><path d="M12 11v6" /><path d="M12 7h.01" />
  {:else if name === 'power'}
    <path d="M12 2v10" /><path d="M7.1 5.4a8 8 0 1 0 9.8 0" />
  {:else if name === 'check'}
    <path d="m5 12 4 4L19 6" />
  {:else if name === 'close'}
    <path d="M6.5 6.5 17.5 17.5M17.5 6.5 6.5 17.5" />
  {:else if name === 'warning'}
    <path d="M12 3 2.8 20h18.4Z" /><path d="M12 9v4" /><path d="M12 17h.01" />
  {:else if name === 'flame-filled'}
    <path
      fill="currentColor"
      stroke="none"
      d="M13.7 2.3c.5 3.4-1 5-2.5 6.5-1.2 1.2-2.3 2.3-2.3 4.2 0 1.2.5 2.2 1.4 2.9-.1-1.9.8-3.4 2.3-4.7.2 1.7 1.2 2.8 2.1 3.8.8.9 1.4 1.8 1.4 3 0 2.2-1.8 4-4.1 4-4.5 0-8.1-3.5-8.1-7.8 0-3.7 2.1-6.2 4.4-8.4-.1 2.1.7 3.4 1.7 4.2.2-3.8 1.9-6.2 3.7-7.7Z"
    />
  {:else if name === 'grip-lines'}
    <path d="M7 8h10M7 12h10M7 16h10" />
  {:else if name === 'grip-dots'}
    <g fill="currentColor" stroke="none">
      <circle cx="9" cy="7" r="1.2" /><circle cx="15" cy="7" r="1.2" />
      <circle cx="9" cy="12" r="1.2" /><circle cx="15" cy="12" r="1.2" />
      <circle cx="9" cy="17" r="1.2" /><circle cx="15" cy="17" r="1.2" />
    </g>
  {:else if name === 'star' || name === 'star-filled'}
    <path
      d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-2.9-5.6 2.9 1.1-6.2L3 9.6l6.2-.9Z"
      fill={name === 'star-filled' ? 'currentColor' : 'none'}
    />
  {/if}
</svg>

<style>
  .symbol-icon {
    display: block;
    flex: 0 0 auto;
    pointer-events: none;
  }
</style>
