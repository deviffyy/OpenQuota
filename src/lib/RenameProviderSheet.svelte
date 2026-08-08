<script lang="ts">
  import { onMount, tick } from 'svelte';

  interface Props {
    initialValue: string;
    onRename: (name: string) => void;
    onCancel: () => void;
  }

  let { initialValue, onRename, onCancel }: Props = $props();
  let draft = $state('');
  let sheet = $state<HTMLElement>();
  let input = $state<HTMLInputElement>();

  onMount(() => {
    draft = initialValue;
    void tick().then(() => {
      input?.focus();
      input?.select();
    });
  });

  function handleKeydown(event: KeyboardEvent) {
    event.stopPropagation();
    if (event.key === 'Escape') {
      event.preventDefault();
      onCancel();
      return;
    }
    if (event.key !== 'Tab' || !sheet) return;
    const controls = [
      ...sheet.querySelectorAll<HTMLElement>('input:not(:disabled), button:not(:disabled)'),
    ];
    if (controls.length === 0) return;
    const first = controls[0];
    const last = controls.at(-1)!;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<div
  class="rename-backdrop"
  role="presentation"
  onpointerdown={(event) => event.stopPropagation()}
  onclick={(event) => event.stopPropagation()}
  onkeydown={handleKeydown}
>
  <div
    class="rename-sheet"
    bind:this={sheet}
    role="dialog"
    aria-modal="true"
    aria-labelledby="rename-title"
    aria-describedby="rename-message"
  >
    <form
      onsubmit={(event) => {
        event.preventDefault();
        onRename(draft);
      }}
    >
      <h1 id="rename-title">Rename Card</h1>
      <input
        bind:this={input}
        bind:value={draft}
        type="text"
        maxlength="48"
        placeholder="Name"
        aria-label="Name"
      />
      <p id="rename-message">Leave the name empty to go back to the default.</p>
      <div class="rename-sheet__actions">
        <button type="button" onclick={onCancel}>Cancel</button>
        <button class="rename-sheet__confirm" type="submit">Rename</button>
      </div>
    </form>
  </div>
</div>

<style>
  .rename-backdrop {
    position: absolute;
    z-index: 120;
    display: grid;
    padding: 48px 18px 18px;
    background: color-mix(in srgb, #000 24%, transparent);
    backdrop-filter: blur(7px) saturate(0.9);
    animation: rename-backdrop-in var(--motion-switch) both;
    inset: 0;
    place-items: start center;
  }

  .rename-sheet {
    width: min(252px, 100%);
    box-sizing: border-box;
    padding: 17px;
    border: 1px solid color-mix(in srgb, var(--text) 11%, transparent);
    border-radius: 15px;
    color: var(--text);
    background: color-mix(in srgb, var(--tray) 96%, transparent);
    box-shadow:
      0 22px 60px rgba(0, 0, 0, 0.3),
      0 2px 8px rgba(0, 0, 0, 0.14);
    backdrop-filter: blur(24px) saturate(1.18);
    animation: rename-sheet-in var(--motion-spring) both;
  }

  form {
    display: grid;
    gap: 10px;
  }

  h1 {
    margin: 0;
    font-size: 13px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }

  input {
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    padding: 7px 9px;
    border: 1px solid var(--separator);
    border-radius: 8px;
    outline: none;
    color: var(--text);
    background: var(--card);
    font: inherit;
    font-size: 12px;
  }

  input:focus {
    border-color: var(--meter-fill);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--meter-fill) 20%, transparent);
  }

  p {
    margin: -2px 0 0;
    color: var(--secondary);
    font-size: 11px;
    line-height: 15px;
  }

  .rename-sheet__actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 6px;
  }

  .rename-sheet__actions button {
    min-width: 76px;
    min-height: 31px;
    padding: 6px 12px;
    border: 1px solid color-mix(in srgb, var(--text) 9%, transparent);
    border-radius: 8px;
    color: var(--text);
    background: color-mix(in srgb, var(--text) 7%, var(--tray));
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }

  .rename-sheet__actions button:hover {
    background: color-mix(in srgb, var(--text) 11%, var(--tray));
  }

  .rename-sheet__actions button:active {
    transform: scale(0.97);
  }

  .rename-sheet__actions button:focus-visible {
    outline: 2px solid color-mix(in srgb, var(--meter-fill) 65%, transparent);
    outline-offset: 2px;
  }

  .rename-sheet__actions .rename-sheet__confirm {
    border-color: color-mix(in srgb, var(--meter-fill) 80%, #000);
    color: #fff;
    background: var(--meter-fill);
    box-shadow: 0 1px 3px color-mix(in srgb, var(--meter-fill) 34%, transparent);
  }

  .rename-sheet__actions .rename-sheet__confirm:hover {
    background: color-mix(in srgb, var(--meter-fill) 88%, #000);
  }

  @keyframes rename-backdrop-in {
    from {
      background-color: transparent;
      backdrop-filter: blur(0) saturate(1);
    }
  }

  @keyframes rename-sheet-in {
    from {
      opacity: 0;
      transform: translateY(-12px) scale(0.975);
    }
  }

  :global(:root[data-reduced-motion]) .rename-backdrop,
  :global(:root[data-reduced-motion]) .rename-sheet {
    animation-duration: 0ms;
  }
</style>
