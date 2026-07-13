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
  let listbox = $state<HTMLDivElement>();
  let open = $state(false);
  let openAbove = $state(false);
  let maxListHeight = $state(240);
  let listTop = $state(8);
  let listLeft = $state(8);
  let triggerWidth = $state(132);
  let positioned = $state(false);
  const listboxId = `select-menu-${Math.random().toString(36).slice(2)}`;
  const selectedLabel = $derived(options.find((option) => option.value === value)?.label ?? value);

  onMount(() => {
    const closeOutside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (root && !root.contains(target) && !listbox?.contains(target)) open = false;
    };
    const reposition = () => {
      if (open) placeListbox();
    };
    document.addEventListener('pointerdown', closeOutside);
    document.addEventListener('scroll', reposition, true);
    window.addEventListener('resize', reposition);
    return () => {
      document.removeEventListener('pointerdown', closeOutside);
      document.removeEventListener('scroll', reposition, true);
      window.removeEventListener('resize', reposition);
    };
  });

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  function placeListbox(estimatedHeight = listbox?.scrollHeight || options.length * 24 + 12) {
    const trigger = root?.querySelector<HTMLButtonElement>('[role="combobox"]');
    if (!root || !trigger) return;

    const triggerBounds = trigger.getBoundingClientRect();
    const boundaryTop = 8;
    const boundaryBottom = window.innerHeight - 8;
    const gap = 5;
    const spaceAbove = Math.max(0, triggerBounds.top - boundaryTop - gap);
    const spaceBelow = Math.max(0, boundaryBottom - triggerBounds.bottom - gap);

    openAbove = spaceBelow < Math.min(estimatedHeight, 120) && spaceAbove > spaceBelow;
    maxListHeight = Math.max(48, Math.floor(openAbove ? spaceAbove : spaceBelow));
    triggerWidth = Math.round(triggerBounds.width);

    const renderedHeight = Math.min(estimatedHeight, maxListHeight);
    const renderedWidth =
      listbox?.offsetWidth || Math.max(triggerBounds.width, variant === 'title' ? 138 : 132);
    const preferredTop = openAbove
      ? triggerBounds.top - gap - renderedHeight
      : triggerBounds.bottom + gap;
    const preferredLeft =
      variant === 'title' ? triggerBounds.left - 5 : triggerBounds.right - renderedWidth;
    listTop = Math.round(
      Math.min(Math.max(boundaryTop, preferredTop), boundaryBottom - renderedHeight),
    );
    listLeft = Math.round(
      Math.min(Math.max(8, preferredLeft), window.innerWidth - 8 - renderedWidth),
    );
    positioned = true;
  }

  async function openAndFocus(position: 'selected' | 'first' | 'last' = 'selected') {
    positioned = false;
    open = true;
    await tick();
    placeListbox();
    const items = listbox?.querySelectorAll<HTMLButtonElement>('[role="option"]');
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
    const items = [...(listbox?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? [])];
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
        transform: translateY(${(1 - progress) * (openAbove ? 4 : -4)}px) scale(${0.96 + progress * 0.04});
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
      class:select-menu__list--above={openAbove}
      class:select-menu__list--title={variant === 'title'}
      class:select-menu__list--pending={!positioned}
      style={`--select-menu-max-height: ${maxListHeight}px; --select-menu-trigger-width: ${triggerWidth}px; top: ${listTop}px; left: ${listLeft}px`}
      bind:this={listbox}
      use:portal
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

<style>
  :global {
    .select-menu {
      position: relative;
      min-width: 112px;
      flex: 0 0 auto;
    }

    .select-menu__trigger {
      display: flex;
      width: 100%;
      min-height: 28px;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      padding: 4px 8px 4px 10px;
      border: 1px solid var(--separator);
      border-radius: 7px;
      color: var(--text);
      background: var(--card);
      font-size: 12px;
      line-height: 18px;
      cursor: pointer;
      transition:
        border-color var(--motion-switch),
        background var(--motion-switch),
        box-shadow var(--motion-switch);
    }

    .select-menu__trigger:hover,
    .select-menu__trigger[aria-expanded='true'] {
      border-color: color-mix(in srgb, var(--text) 18%, transparent);
      background: var(--card-hover);
    }

    .select-menu__trigger:focus-visible {
      outline: 2px solid color-mix(in srgb, var(--meter-fill) 55%, transparent);
      outline-offset: 1px;
    }

    .select-menu__trigger[aria-expanded='true'] .symbol-icon {
      transform: rotate(180deg);
    }

    .select-menu__list {
      position: fixed;
      z-index: 1000;
      width: max-content;
      min-width: max(var(--select-menu-trigger-width), 132px);
      max-width: min(240px, calc(100vw - 16px));
      max-height: min(var(--select-menu-max-height), calc(100vh - 16px));
      padding: 5px;
      overflow-y: auto;
      overscroll-behavior: contain;
      border: 1px solid var(--separator);
      border-radius: 8px;
      background: color-mix(in srgb, var(--tray) 90%, transparent);
      box-shadow:
        0 12px 32px rgba(0, 0, 0, 0.2),
        0 2px 8px rgba(0, 0, 0, 0.1);
      backdrop-filter: blur(22px) saturate(1.35);
      transform-origin: top right;
      will-change: transform, opacity;
    }

    .select-menu__list--above {
      transform-origin: bottom right;
    }

    .select-menu__list--pending {
      visibility: hidden;
    }

    .select-menu__list button {
      display: flex;
      width: 100%;
      min-height: 24px;
      align-items: center;
      justify-content: flex-start;
      gap: 5px;
      padding: 3px 8px 3px 3px;
      border: 0;
      border-radius: 6px;
      color: var(--text);
      background: transparent;
      font-size: 13px;
      line-height: 17px;
      text-align: left;
      cursor: pointer;
    }

    .select-menu__list button:hover,
    .select-menu__list button:focus-visible {
      outline: none;
      color: #ffffff;
      background: var(--meter-fill);
    }

    .select-menu__list button.active {
      font-weight: 400;
    }

    .select-menu__list button > span:last-child {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .select-menu__check {
      display: grid;
      width: 16px;
      height: 16px;
      flex: 0 0 16px;
      place-items: center;
    }

    :root[data-density='compact'] .select-menu__trigger {
      min-height: 26px;
      padding: 3px 7px 3px 9px;
      font-size: 12px;
      line-height: 16px;
    }
  }
</style>
