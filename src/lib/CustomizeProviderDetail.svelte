<script lang="ts">
  import { onDestroy } from 'svelte';
  import { flip } from 'svelte/animate';
  import type { ProviderCatalogIndex } from './metrics';
  import type { AppSettings, MetricLayout, MetricSection, ProviderLayout } from './types';
  import Icon from './Icon.svelte';
  import { reorderFlip } from './motion';
  import { pointerReorder } from './pointerReorder';

  interface Props {
    settings: AppSettings;
    providerId: string;
    catalog: ProviderCatalogIndex;
    onChange: (settings: AppSettings) => void;
    onReorderStart: () => void;
    onReorderEnd: (moved: boolean, cancelled?: boolean) => void;
    reducedMotion: boolean;
  }
  let {
    settings,
    providerId,
    catalog,
    onChange,
    onReorderStart,
    onReorderEnd,
    reducedMotion,
  }: Props = $props();
  const metricDefinition = (id: string) => catalog.metric(id);
  const providerDisplayName = (id: string) => catalog.displayName(id);
  let message = $state('');
  let messageKind = $state<'success' | 'denied'>('success');
  let messageTimer: ReturnType<typeof setTimeout> | undefined;
  onDestroy(() => {
    if (messageTimer) clearTimeout(messageTimer);
  });
  const provider = $derived(settings.providers.find((item) => item.id === providerId));

  function updateProvider(next: ProviderLayout) {
    onChange({
      ...settings,
      providers: settings.providers.map((item) => (item.id === next.id ? next : item)),
    });
  }
  function updateMetric(metric: MetricLayout) {
    if (!provider) return;
    updateProvider({
      ...provider,
      metrics: provider.metrics.map((item) => (item.id === metric.id ? metric : item)),
    });
  }
  function togglePin(metric: MetricLayout, button: HTMLButtonElement) {
    if (!provider || !metricDefinition(metric.id)?.pinnable) return;
    if (!metric.pinned && provider.metrics.filter((item) => item.pinned).length >= 2) {
      showMessage('Up to 2 stars per provider', 'denied');
      if (!reducedMotion) {
        button.animate?.(
          [
            { transform: 'translateX(0)' },
            { transform: 'translateX(5px)' },
            { transform: 'translateX(-5px)' },
            { transform: 'translateX(5px)' },
            { transform: 'translateX(-5px)' },
            { transform: 'translateX(5px)' },
            { transform: 'translateX(0)' },
          ],
          { duration: 400, delay: 100 },
        );
      }
      return;
    }
    showMessage(metric.pinned ? 'Removed from menu bar' : 'Starred for menu bar', 'success');
    updateMetric({ ...metric, pinned: !metric.pinned });
  }
  function showMessage(text: string, kind: 'success' | 'denied') {
    message = text;
    messageKind = kind;
    if (messageTimer) clearTimeout(messageTimer);
    messageTimer = setTimeout(() => (message = ''), 2500);
  }
  function reorder(
    draggedId: string,
    target: MetricLayout,
    section: MetricSection = target.section,
  ) {
    if (!provider || draggedId === target.id) return;
    const metrics = [...provider.metrics];
    const from = metrics.findIndex((metric) => metric.id === draggedId);
    const to = metrics.findIndex((metric) => metric.id === target.id);
    if (from < 0 || to < 0) return;
    const [source] = metrics.splice(from, 1);
    const moved = { ...source, section };
    metrics.splice(to, 0, moved);
    updateProvider({ ...provider, metrics });
  }
  function moveIntoSection(draggedId: string, section: MetricSection) {
    if (!provider) return;
    const metrics = [...provider.metrics];
    const from = metrics.findIndex((metric) => metric.id === draggedId);
    if (from < 0) return;
    const [source] = metrics.splice(from, 1);
    const moved = { ...source, section };
    const lastInSection = metrics.reduce(
      (last, metric, index) => (metric.section === section ? index : last),
      -1,
    );
    const insertAt =
      lastInSection >= 0 ? lastInSection + 1 : section === 'alwaysVisible' ? 0 : metrics.length;
    metrics.splice(insertAt, 0, moved);
    updateProvider({ ...provider, metrics });
  }
</script>

{#if provider}
  <section
    class="screen customize-detail"
    aria-label={`Customize ${providerDisplayName(provider.id)}`}
  >
    {#each ['alwaysVisible', 'onDemand'] as section (section)}
      {@const sectionMetrics = provider.metrics.filter((metric) => metric.section === section)}
      <div
        class="metric-section"
        role="group"
        aria-label={section === 'alwaysVisible' ? 'Always Visible metrics' : 'On Demand metrics'}
      >
        <h2>{section === 'alwaysVisible' ? 'Always Visible' : 'On Demand'}</h2>
        <div class="metric-list" role="list">
          {#if sectionMetrics.length === 0}
            <div
              class="empty-drop-zone"
              role="listitem"
              data-reorder-group={`customize-metrics:${provider.id}`}
              data-reorder-id={`section:${section}`}
            >
              Drag metrics here
            </div>
          {/if}
          {#each sectionMetrics as metric (metric.id)}
            <div
              role="listitem"
              class:disabled={!metric.enabled}
              class="customize-metric-row"
              data-reorder-group={`customize-metrics:${provider.id}`}
              data-reorder-id={metric.id}
              use:pointerReorder={{
                id: metric.id,
                group: `customize-metrics:${provider.id}`,
                label: metricDefinition(metric.id)?.label ?? metric.id,
                gripOnly: true,
                touchGripOnly: true,
                onReorder: (targetId) => {
                  if (targetId.startsWith('section:')) {
                    moveIntoSection(metric.id, targetId.slice(8) as MetricSection);
                    return;
                  }
                  const target = provider.metrics.find((item) => item.id === targetId);
                  if (target) reorder(metric.id, target, target.section);
                },
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
                tabindex="0"
                aria-label={`Move ${metricDefinition(metric.id)?.label ?? metric.id}`}
                aria-describedby="reorder-instructions"
                aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
                ><Icon name="grip-lines" size={16} strokeWidth={2} /></span
              >
              <span class="customize-metric-name"
                >{metricDefinition(metric.id)?.label ?? metric.id}</span
              >
              <span class="customize-metric-pin-slot">
                {#if metricDefinition(metric.id)?.pinnable}<button
                    class:pinned={metric.pinned}
                    class="pin-button"
                    type="button"
                    aria-label={`${metric.pinned ? 'Unpin' : 'Pin'} ${metricDefinition(metric.id)?.label}`}
                    onclick={(event) => togglePin(metric, event.currentTarget)}
                    ><Icon
                      name={metric.pinned ? 'star-filled' : 'star'}
                      size={15}
                      strokeWidth={1.7}
                    /></button
                  >{/if}
              </span>
              <label class="switch"
                ><input
                  aria-label={`Show ${metricDefinition(metric.id)?.label ?? metric.id}`}
                  type="checkbox"
                  checked={metric.enabled}
                  onchange={(event) =>
                    updateMetric({
                      ...metric,
                      enabled: event.currentTarget.checked,
                    })}
                /><span></span></label
              >
            </div>
          {/each}
        </div>
      </div>
    {/each}
    {#if message}
      <div class:denied={messageKind === 'denied'} class="customization-pill" role="status">
        <Icon
          name={messageKind === 'denied' ? 'about' : 'check'}
          size={15}
          strokeWidth={2.2}
        />{message}
      </div>
    {/if}
  </section>
{/if}

<style>
  :global {
    .metric-section {
      margin-top: 4px;
    }

    .customize-metric-row {
      display: flex;
      min-height: 39px;
      align-items: center;
      gap: 4px;
      padding: 4px 6px;
      border-top: 1px solid var(--separator);
    }

    .customize-metric-row:first-child {
      border-top: 0;
    }

    .customize-metric-row.disabled {
      opacity: 0.55;
    }

    .customize-metric-row > label {
      display: flex;
      min-width: 0;
      flex: 1;
      align-items: center;
      gap: 5px;
      font-size: 10px;
    }

    .pin-button.pinned {
      color: var(--meter-fill);
    }

    .metric-section {
      margin-top: 0;
      margin-bottom: 14px;
    }

    .customize-metric-row {
      min-height: 42px;
      gap: 10px;
      padding: 9px 12px;
    }

    .customize-metric-row > label {
      font-size: 12px;
    }

    .customize-metric-name {
      min-width: 0;
      flex: 1;
      overflow: hidden;
      font-size: 13px;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .customize-metric-pin-slot {
      display: grid;
      width: 25px;
      height: 25px;
      flex: 0 0 25px;
      place-items: center;
    }

    .customize-metric-row > .switch {
      display: block;
      flex: 0 0 28px;
    }

    .empty-drop-zone {
      display: grid;
      height: 30px;
      margin: 8px;
      border: 1px dashed var(--separator);
      border-radius: 8px;
      color: var(--tertiary);
      font-size: 10px;
      place-items: center;
    }

    .customization-pill {
      position: sticky;
      bottom: 8px;
      z-index: 20;
      display: flex;
      width: max-content;
      max-width: calc(100% - 16px);
      align-items: center;
      gap: 6px;
      margin: 8px auto 0;
      padding: 7px 10px;
      border: 1px solid var(--separator);
      border-radius: 999px;
      color: var(--text);
      background: color-mix(in srgb, var(--tray) 96%, transparent);
      box-shadow: 0 8px 22px rgba(0, 0, 0, 0.22);
      font-size: 10px;
      animation: detail-in var(--motion-spring) both;
    }

    .customization-pill .symbol-icon {
      color: #34c759;
    }

    .customization-pill.denied {
      color: var(--warning);
      animation: detail-in var(--motion-spring) both;
    }

    .customization-pill.denied .symbol-icon {
      color: var(--warning);
    }
  }
</style>
