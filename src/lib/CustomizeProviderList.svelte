<script lang="ts">
  import type { AppSettings, ProviderLayout } from './types';
  import { providerDisplayName } from './metrics';
  import Icon from './Icon.svelte';
  import ProviderIcon from './ProviderIcon.svelte';
  import { beginDrag } from './dragPreview';

  interface Props {
    settings: AppSettings;
    onOpen: (providerId: string) => void;
    onChange: (settings: AppSettings) => void;
    onSettings: () => void;
  }
  let { settings, onOpen, onChange, onSettings }: Props = $props();
  let dragged = $state<string | null>(null);

  function updateProvider(provider: ProviderLayout) {
    onChange({
      ...settings,
      providers: settings.providers.map((item) => (item.id === provider.id ? provider : item)),
    });
  }
  function drop(target: string) {
    if (!dragged || dragged === target) return;
    const providers = [...settings.providers];
    const from = providers.findIndex((provider) => provider.id === dragged);
    const to = providers.findIndex((provider) => provider.id === target);
    const [provider] = providers.splice(from, 1);
    providers.splice(to, 0, provider);
    onChange({ ...settings, providers });
    dragged = null;
  }
</script>

<section class="screen customize-screen" aria-label="Customize">
  <div class="customize-list" role="list">
    {#each settings.providers as provider (provider.id)}
      <div
        role="listitem"
        class:inactive={!provider.enabled}
        class:dragging={dragged === provider.id}
        class="provider-list-row"
        draggable={provider.enabled}
        ondragstart={(event) => {
          if (!provider.enabled) return;
          dragged = provider.id;
          beginDrag(
            event,
            providerDisplayName(provider.id),
            `${provider.metrics.filter((metric) => metric.enabled).length} metrics`,
          );
        }}
        ondragend={() => (dragged = null)}
        ondragover={(event) => event.preventDefault()}
        ondrop={() => drop(provider.id)}
      >
        <span class="reorder-grip" aria-hidden="true"
          ><Icon name="grip-lines" size={16} strokeWidth={2} /></span
        >
        <button class="provider-list-main" type="button" onclick={() => onOpen(provider.id)}
          ><ProviderIcon providerId={provider.id} /><span
            ><b>{providerDisplayName(provider.id)}</b><small
              >{provider.metrics.length} metrics</small
            ></span
          ></button
        >
        <label class="switch"
          ><input
            aria-label={`Enable ${provider.id}`}
            type="checkbox"
            checked={provider.enabled}
            onchange={(event) =>
              updateProvider({ ...provider, enabled: event.currentTarget.checked })}
          /><span></span></label
        >
        <button
          class="chevron"
          type="button"
          aria-label={`Customize ${provider.id}`}
          onclick={() => onOpen(provider.id)}
          ><Icon name="chevron-right" size={13} strokeWidth={2.2} /></button
        >
      </div>
    {/each}
  </div>
  <button class="screen-cross-link" type="button" aria-label="Settings" onclick={onSettings}>
    <Icon name="gear" size={17} />
    <span><b>Settings</b><small>Notifications, appearance and more</small></span>
    <Icon name="chevron-right" size={13} strokeWidth={2.2} />
  </button>
</section>
