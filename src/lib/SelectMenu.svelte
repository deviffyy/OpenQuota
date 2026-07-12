<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { cubicOut } from 'svelte/easing';
  import type { TransitionConfig } from 'svelte/transition';
  import Icon from './Icon.svelte';

  export interface SelectOption {
    value: string;
    label: string;
  }

  interface Props {
    label: string;
    value: string;
    options: SelectOption[];
    onChange: (value: string) => void;
    variant?: 'field' | 'title';
  }

  let { label, value, options, onChange, variant = 'field' }: Props = $props();
  let root = $state<HTMLDivElement>();
  let open = $state(false);
  const listboxId = `select-menu-${Math.random().toString(36).slice(2)}`;
  const selectedLabel = $derived(options.find((option) => option.value === value)?.label ?? value);

  onMount(() => {
    const closeOutside = (event: PointerEvent) => {
      if (root && !root.contains(event.target as Node)) open = false;
    };
    document.addEventListener('pointerdown', closeOutside);
    return () => document.removeEventListener('pointerdown', closeOutside);
  });

  async function openAndFocus(position: 'selected' | 'first' | 'last' = 'selected') {
    open = true;
    await tick();
    const items = root?.querySelectorAll<HTMLButtonElement>('[role="option"]');
    if (!items?.length) return;
    const selected = options.findIndex((option) => option.value === value);
    const index = position === 'first' ? 0 : position === 'last' ? items.length - 1 : selected;
    items[Math.max(0, index)]?.focus();
  }

  function choose(next: string) {
    open = false;
    if (next !== value) onChange(next);
    root?.querySelector<HTMLButtonElement>('[role="combobox"]')?.focus();
  }

  function triggerKeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown' || event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      void openAndFocus(event.key === 'ArrowDown' ? 'selected' : 'first');
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      void openAndFocus('last');
    }
  }

  function optionKeydown(event: KeyboardEvent) {
    const item = event.currentTarget as HTMLButtonElement;
    if (event.key === 'Escape' || event.key === 'Tab') {
      open = false;
      if (event.key === 'Escape') {
        event.preventDefault();
        root?.querySelector<HTMLButtonElement>('[role="combobox"]')?.focus();
      }
      return;
    }
    if (
      event.key !== 'ArrowDown' &&
      event.key !== 'ArrowUp' &&
      event.key !== 'Home' &&
      event.key !== 'End'
    )
      return;
    event.preventDefault();
    const items = [...(root?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? [])];
    const current = items.indexOf(item);
    const index =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? items.length - 1
          : event.key === 'ArrowDown'
            ? (current + 1) % items.length
            : (current - 1 + items.length) % items.length;
    items[index]?.focus();
  }

  function menuMotion(duration: number): TransitionConfig {
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    return {
      duration: reducedMotion ? 0 : duration,
      easing: cubicOut,
      css: (progress) => `
        opacity: ${progress};
        transform: translateY(${(1 - progress) * -4}px) scale(${0.96 + progress * 0.04});
      `,
    };
  }

  function menuReveal(node: Element): TransitionConfig {
    void node;
    return menuMotion(150);
  }

  function menuDismiss(node: Element): TransitionConfig {
    void node;
    return menuMotion(100);
  }
</script>

<div class="select-menu" class:select-menu--title={variant === 'title'} bind:this={root}>
  <button
    type="button"
    class="select-menu__trigger"
    role="combobox"
    aria-label={label}
    aria-controls={listboxId}
    aria-expanded={open}
    aria-haspopup="listbox"
    onclick={() => (open ? (open = false) : void openAndFocus())}
    onkeydown={triggerKeydown}
  >
    <span>{selectedLabel}</span><Icon name="chevron-down" size={12} strokeWidth={2} />
  </button>
  {#if open}
    <div
      class="select-menu__list"
      id={listboxId}
      role="listbox"
      aria-label={label}
      in:menuReveal
      out:menuDismiss
    >
      {#each options as option (option.value)}
        <button
          type="button"
          role="option"
          aria-selected={option.value === value}
          class:active={option.value === value}
          onclick={() => choose(option.value)}
          onkeydown={optionKeydown}
        >
          <span class="select-menu__check"
            >{#if option.value === value}<Icon
                name="check"
                size={12}
                strokeWidth={2.2}
              />{/if}</span
          >
          <span>{option.label}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>
