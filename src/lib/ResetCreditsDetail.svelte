<script lang="ts">
  import { SvelteMap } from 'svelte/reactivity';
  import { claimCodexResetCredit } from './backend';
  import { formatReset } from './pacing';
  import type { ResetClaimOutcome } from './types';

  interface Props {
    title: string;
    count: number;
    expiries: string[];
    now: number;
    timeFormat: 'system' | 'twelveHour' | 'twentyFourHour';
    top: number;
    onEnter: () => void;
    onLeave: () => void;
    onDismiss: () => void;
  }

  let { title, count, expiries, now, timeFormat, top, onEnter, onLeave, onDismiss }: Props =
    $props();
  let confirmingExpiry = $state<string | null>(null);
  let pendingExpiry = $state<string | null>(null);
  let result = $state<{ expiry: string; outcome: ResetClaimOutcome } | null>(null);
  const requestIds = new SvelteMap<string, string>();

  const entries = $derived(
    [...expiries].sort().map((expiry, index) => {
      const timestamp = new Date(expiry).getTime();
      const remaining = timestamp - now;
      const severity =
        remaining <= 48 * 60 * 60 * 1000
          ? 'critical'
          : remaining <= 7 * 24 * 60 * 60 * 1000
            ? 'warning'
            : 'normal';
      const relative = formatReset(expiry, now, 'countdown', timeFormat).replace(
        /^Resets(?: in)?\s*/,
        '',
      );
      const exact = formatReset(expiry, now, 'exact', timeFormat).replace(/^Resets\s*/, '');
      const imminent = relative === 'soon';
      return {
        id: `${expiry}:${index}`,
        expiry,
        number: index + 1,
        severity,
        exact: imminent ? 'Expiring soon' : exact,
        relative: imminent ? null : relative,
      };
    }),
  );

  const resultMessage = $derived(
    result?.outcome === 'success'
      ? 'Reset applied.'
      : result?.outcome === 'nothingToReset'
        ? 'No active limit needs resetting.'
        : result?.outcome === 'noCredit'
          ? 'This reset is no longer available.'
          : result?.outcome === 'failed'
            ? 'Could not use this reset. Try again.'
            : null,
  );

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape') return;
    if (confirmingExpiry) {
      confirmingExpiry = null;
      return;
    }
    if (!pendingExpiry) onDismiss();
  }

  function beginClaim(expiry: string) {
    confirmingExpiry = expiry;
    result = null;
    if (!requestIds.has(expiry)) {
      requestIds.set(
        expiry,
        globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`,
      );
    }
  }

  async function confirmClaim(expiry: string) {
    const requestId = requestIds.get(expiry);
    if (!requestId || pendingExpiry) return;
    pendingExpiry = expiry;
    let outcome: ResetClaimOutcome;
    try {
      outcome = await claimCodexResetCredit(expiry, requestId);
    } catch {
      outcome = 'failed';
    }
    result = { expiry, outcome };
    pendingExpiry = null;
    confirmingExpiry = null;
    if (outcome !== 'failed') requestIds.delete(expiry);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="reset-credits-detail"
  style={`top:${top}px`}
  role="dialog"
  tabindex="-1"
  aria-label={`${title} details`}
  onmouseenter={onEnter}
  onfocusin={onEnter}
  onmouseleave={() => {
    if (!confirmingExpiry && !pendingExpiry) onLeave();
  }}
>
  <div class="reset-detail-body">
    {#if resultMessage}
      <div
        class:reset-result-error={result?.outcome === 'failed'}
        class="reset-result"
        role="status"
      >
        {resultMessage}
      </div>
    {/if}
    {#if entries.length > 0}
      <div class="reset-timeline">
        {#each entries as entry, index (entry.id)}
          <div class="reset-entry-shell">
            <div class="reset-rail" aria-hidden="true">
              <i class:reset-rail-hidden={index === 0}></i>
              <b class="reset-node reset-node--{entry.severity}">{entry.number}</b>
              <i class:reset-rail-hidden={index === entries.length - 1}></i>
            </div>
            <div class="reset-entry-content">
              {#if confirmingExpiry === entry.expiry}
                <div class="reset-confirm">
                  <strong>Use this reset?</strong>
                  <span>Immediately reset your usage limits. This can't be undone.</span>
                  <div>
                    <button
                      class="reset-confirm-primary"
                      type="button"
                      disabled={pendingExpiry !== null}
                      onclick={() => confirmClaim(entry.expiry)}
                      >{pendingExpiry === entry.expiry ? 'Resetting…' : 'Use reset'}</button
                    >
                    <button
                      type="button"
                      disabled={pendingExpiry !== null}
                      onclick={() => (confirmingExpiry = null)}>Cancel</button
                    >
                  </div>
                </div>
              {:else}
                <div class="reset-entry">
                  <span>{entry.exact}</span>
                  <div class="reset-trailing">
                    {#if entry.relative}<small>{entry.relative}</small>{/if}
                    <button
                      class="reset-use"
                      type="button"
                      aria-label={`Use reset expiring ${entry.exact}`}
                      disabled={pendingExpiry !== null}
                      onclick={() => beginClaim(entry.expiry)}>Use</button
                    >
                  </div>
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {:else if count > 0}
      <div class="reset-empty">
        <strong>{count} available</strong>
        <span>Expiry times unavailable</span>
      </div>
    {:else}
      <div class="reset-empty"><span>No rate limit resets available</span></div>
    {/if}
  </div>
</div>

<style>
  :global {
    .reset-credits-detail {
      position: fixed;
      right: 8px;
      z-index: 100;
      box-sizing: border-box;
      width: 250px;
      max-height: calc(100vh - 16px);
      overflow: hidden;
      border: 1px solid var(--separator);
      border-radius: 11px;
      color: var(--text);
      background: color-mix(in srgb, var(--tray) 98%, transparent);
      box-shadow:
        0 10px 28px rgba(0, 0, 0, 0.22),
        0 2px 7px rgba(0, 0, 0, 0.12);
      backdrop-filter: blur(16px);
      animation: reset-detail-in 120ms ease-out both;
    }

    .reset-detail-body {
      max-height: calc(100vh - 16px);
      padding: 14px;
      overflow-y: auto;
    }

    .reset-timeline {
      display: grid;
    }

    .reset-result {
      margin-bottom: 8px;
      padding: 7px 8px;
      border-radius: 7px;
      color: var(--text);
      background: color-mix(in srgb, var(--meter-fill) 16%, transparent);
      font-size: 10px;
    }

    .reset-result-error {
      color: var(--error);
      background: var(--error-bg);
    }

    .reset-entry-shell {
      display: grid;
      grid-template-columns: 18px minmax(0, 1fr);
      gap: 10px;
    }

    .reset-entry-content {
      min-width: 0;
      padding: 3px 0;
    }

    .reset-entry {
      display: flex;
      height: 30px;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      font-size: 11px;
      line-height: 14px;
    }

    .reset-entry > span {
      overflow: hidden;
      font-weight: 550;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .reset-trailing {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: flex-end;
    }

    .reset-trailing > small {
      color: var(--secondary);
      font: inherit;
      font-variant-numeric: tabular-nums;
      white-space: nowrap;
      transition: opacity 100ms ease;
    }

    .reset-use,
    .reset-confirm button {
      min-height: 22px;
      padding: 2px 8px;
      border: 1px solid var(--separator);
      border-radius: 6px;
      color: var(--text);
      background: var(--card);
      font: inherit;
      font-size: 10px;
      cursor: default;
    }

    .reset-use {
      position: absolute;
      right: 0;
      opacity: 0;
      transition: opacity 100ms ease;
    }

    .reset-entry:hover .reset-trailing > small,
    .reset-entry:focus-within .reset-trailing > small {
      opacity: 0;
    }

    .reset-entry:hover .reset-use,
    .reset-entry:focus-within .reset-use {
      opacity: 1;
    }

    .reset-use:disabled,
    .reset-confirm button:disabled {
      opacity: 0.55;
    }

    .reset-confirm {
      display: grid;
      gap: 8px;
      margin: 1px 0 5px;
      padding: 10px;
      border-radius: 8px;
      background: color-mix(in srgb, var(--text) 4%, transparent);
      font-size: 10px;
      animation: reset-confirm-in 150ms ease-out both;
    }

    .reset-confirm > span {
      color: var(--secondary);
      line-height: 14px;
    }

    .reset-confirm > div {
      display: flex;
      gap: 8px;
    }

    .reset-confirm > div > button {
      flex: 1;
    }

    .reset-confirm .reset-confirm-primary {
      color: white;
      background: var(--meter-fill);
      font-weight: 600;
    }

    .reset-rail {
      display: grid;
      height: 100%;
      grid-template-rows: 8px 18px 1fr;
      justify-items: center;
      align-items: center;
    }

    .reset-rail > i {
      width: 1px;
      height: 100%;
      background: var(--separator);
    }

    .reset-rail > .reset-rail-hidden {
      opacity: 0;
    }

    .reset-node {
      display: grid;
      width: 18px;
      height: 18px;
      place-items: center;
      border-radius: 50%;
      color: white;
      background: var(--meter-fill);
      font-size: 11px;
      font-weight: 600;
      line-height: 1;
    }

    .reset-node--warning {
      color: #171719;
      background: var(--meter-warning);
    }

    .reset-node--critical {
      background: var(--meter-critical);
    }

    .reset-empty {
      display: grid;
      justify-items: center;
      gap: 4px;
      padding: 12px 4px 8px;
      color: var(--secondary);
      font-size: 11px;
      line-height: 15px;
      text-align: center;
    }

    .reset-empty strong {
      color: var(--text);
      font-weight: 650;
    }

    @keyframes reset-detail-in {
      from {
        opacity: 0;
        transform: translateY(-2px) scale(0.98);
      }
      to {
        opacity: 1;
        transform: translateY(0) scale(1);
      }
    }

    @keyframes reset-confirm-in {
      from {
        opacity: 0;
        transform: scale(0.97);
        transform-origin: top;
      }
      to {
        opacity: 1;
        transform: scale(1);
      }
    }

    :root[data-density='compact'] .reset-entry {
      height: 26px;
      font-size: 10px;
    }
  }
</style>
