<script lang="ts">
  import type { ProviderCatalogIndex } from './metrics';
  import { withProviderName } from './providerNames';
  import type { AppSettings, ProviderLayout } from './types';

  interface Props {
    settings: AppSettings;
    provider: ProviderLayout;
    catalog: ProviderCatalogIndex;
    onChange: (settings: AppSettings) => void;
  }

  let { settings, provider, catalog, onChange }: Props = $props();
  let draft = $state('');
  let focused = $state(false);
  const defaultName = $derived(catalog.displayName(provider.id));

  $effect(() => {
    if (!focused) draft = settings.providerNames[provider.id] ?? '';
  });

  function commit() {
    const changed = withProviderName(settings, provider.id, draft);
    if (changed !== settings) onChange(changed);
  }

  function cancel(input: HTMLInputElement) {
    draft = settings.providerNames[provider.id] ?? '';
    input.blur();
  }
</script>

<section class="provider-name-section" aria-labelledby={`provider-name-title-${provider.id}`}>
  <h2 id={`provider-name-title-${provider.id}`}>Name</h2>
  <div class="provider-name-card">
    <input
      type="text"
      maxlength="48"
      bind:value={draft}
      placeholder={defaultName}
      aria-label={`Name for ${defaultName}`}
      autocomplete="off"
      onfocus={() => (focused = true)}
      onblur={() => {
        commit();
        focused = false;
      }}
      onkeydown={(event) => {
        if (event.key === 'Enter') {
          event.currentTarget.blur();
        } else if (event.key === 'Escape') {
          event.preventDefault();
          event.stopPropagation();
          cancel(event.currentTarget);
        }
      }}
    />
  </div>
</section>

<style>
  .provider-name-section {
    margin-bottom: 14px;
  }

  .provider-name-section h2 {
    margin: 0;
    padding: 0 8px 4px;
    color: var(--secondary);
    font-size: 11px;
    font-weight: 600;
  }

  .provider-name-card {
    overflow: hidden;
    border-radius: 12px;
    background: var(--card);
  }

  .provider-name-card:focus-within {
    box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--meter-fill) 65%, transparent);
  }

  input {
    display: block;
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    padding: 10px 12px;
    border: 0;
    outline: none;
    color: var(--text);
    background: transparent;
    font: inherit;
    font-size: 12px;
  }

  input::placeholder {
    color: var(--tertiary);
    opacity: 1;
  }

  :global(:root[data-density='compact']) input {
    padding-top: 8px;
    padding-bottom: 8px;
  }
</style>
