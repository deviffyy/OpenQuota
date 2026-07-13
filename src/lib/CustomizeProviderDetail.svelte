<script lang="ts">
  import { flip } from 'svelte/animate';
  import { metricDefinition, providerDisplayName } from './metrics';
  import type { AppSettings, MetricLayout, MetricSection, ProviderLayout } from './types';
  import Icon from './Icon.svelte';
  import { reorderFlip } from './motion';
  import { pointerReorder } from './pointerReorder';

  interface Props {
    settings: AppSettings;
    providerId: string;
    onChange: (settings: AppSettings) => void;
    onReorderStart: () => void;
    onReorderEnd: (moved: boolean, cancelled?: boolean) => void;
    reducedMotion: boolean;
  }
  let { settings, providerId, onChange, onReorderStart, onReorderEnd, reducedMotion }: Props =
    $props();
  let message = $state('');
  let messageKind = $state<'success' | 'denied'>('success');
  let messageTimer: ReturnType<typeof setTimeout> | undefined;
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
  function togglePin(metric: MetricLayout) {
    if (!provider || !metricDefinition(metric.id)?.pinnable) return;
    if (!metric.pinned && provider.metrics.filter((item) => item.pinned).length >= 2) {
      showMessage('Up to 2 stars per provider', 'denied');
      return;
    }
    showMessage(metric.pinned ? 'Removed from menu bar' : 'Starred for menu bar', 'success');
    updateMetric({ ...metric, enabled: true, pinned: !metric.pinned });
  }
  function showMessage(text: string, kind: 'success' | 'denied') {
    message = text;
    messageKind = kind;
    if (messageTimer) clearTimeout(messageTimer);
    messageTimer = setTimeout(() => (message = ''), 1800);
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
                    onclick={() => togglePin(metric)}
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
                      pinned: event.currentTarget.checked ? metric.pinned : false,
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
