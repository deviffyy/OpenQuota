<script lang="ts">
  import { flip } from 'svelte/animate';
  import type { AppSettings, ProviderLayout } from './types';
  import { providerDisplayName } from './metrics';
  import Icon from './Icon.svelte';
  import ProviderIcon from './ProviderIcon.svelte';
  import { reorderFlip } from './motion';
  import { pointerReorder } from './pointerReorder';

  interface Props {
    settings: AppSettings;
    onOpen: (providerId: string) => void;
    onChange: (settings: AppSettings) => void;
    onReorderStart: () => void;
    onReorderEnd: (moved: boolean, cancelled?: boolean) => void;
    onSettings: () => void;
    reducedMotion: boolean;
  }
  let {
    settings,
    onOpen,
    onChange,
    onReorderStart,
    onReorderEnd,
    onSettings,
    reducedMotion,
  }: Props = $props();
  function updateProvider(provider: ProviderLayout) {
    onChange({
      ...settings,
      providers: settings.providers.map((item) => (item.id === provider.id ? provider : item)),
    });
  }
  function reorder(draggedId: string, targetId: string) {
    if (draggedId === targetId) return;
    const enabled = settings.providers.filter((provider) => provider.enabled);
    const from = enabled.findIndex((provider) => provider.id === draggedId);
    const to = enabled.findIndex((provider) => provider.id === targetId);
    if (from < 0 || to < 0) return;
    const [provider] = enabled.splice(from, 1);
    enabled.splice(to, 0, provider);
    onChange({
      ...settings,
      providers: [...enabled, ...settings.providers.filter((provider) => !provider.enabled)],
    });
  }
</script>

<section class="screen customize-screen" aria-label="Customize">
  <div class="customize-list" role="list">
    {#each settings.providers as provider (provider.id)}
      <div
        role="listitem"
        class:inactive={!provider.enabled}
        class="provider-list-row"
        data-reorder-group={provider.enabled ? 'customize-providers' : undefined}
        data-reorder-id={provider.enabled ? provider.id : undefined}
        use:pointerReorder={{
          id: provider.id,
          group: 'customize-providers',
          label: providerDisplayName(provider.id),
          disabled: !provider.enabled,
          gripOnly: true,
          touchGripOnly: true,
          onReorder: (targetId) => reorder(provider.id, targetId),
          onStart: onReorderStart,
          onEnd: onReorderEnd,
        }}
        animate:flip={reorderFlip(reducedMotion)}
      >
        <span
          class="reorder-grip"
          data-reorder-handle
          data-reorder-touch-handle
          role="button"
          tabindex={provider.enabled ? 0 : undefined}
          aria-label={`Move ${providerDisplayName(provider.id)}`}
          aria-describedby="reorder-instructions"
          aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
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
