<script lang="ts">
  import { metricDefinition, providerDisplayName } from './metrics';
  import type { AppSettings, MetricLayout, MetricSection, ProviderLayout } from './types';
  import Icon from './Icon.svelte';
  import { beginDrag } from './dragPreview';

  interface Props {
    settings: AppSettings;
    providerId: string;
    onChange: (settings: AppSettings) => void;
  }
  let { settings, providerId, onChange }: Props = $props();
  let message = $state('');
  let messageKind = $state<'success' | 'denied'>('success');
  let messageTimer: ReturnType<typeof setTimeout> | undefined;
  let dragged = $state<string | null>(null);
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
  function drop(target: MetricLayout, section: MetricSection = target.section) {
    if (!provider || !dragged) return;
    const metrics = [...provider.metrics];
    const from = metrics.findIndex((metric) => metric.id === dragged);
    const to = metrics.findIndex((metric) => metric.id === target.id);
    const [moved] = metrics.splice(from, 1);
    moved.section = section;
    metrics.splice(to, 0, moved);
    dragged = null;
    updateProvider({ ...provider, metrics });
  }
  function dropIntoSection(section: MetricSection) {
    if (!provider || !dragged) return;
    const metrics = [...provider.metrics];
    const from = metrics.findIndex((metric) => metric.id === dragged);
    if (from < 0) return;
    const [moved] = metrics.splice(from, 1);
    moved.section = section;
    const lastInSection = metrics.reduce(
      (last, metric, index) => (metric.section === section ? index : last),
      -1,
    );
    metrics.splice(lastInSection + 1, 0, moved);
    dragged = null;
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
        ondragover={(event) => event.preventDefault()}
      >
        <h2>{section === 'alwaysVisible' ? 'Always Visible' : 'On Demand'}</h2>
        <div class="metric-list" role="list">
          {#if sectionMetrics.length === 0}
            <div
              class="empty-drop-zone"
              role="listitem"
              ondragover={(event) => event.preventDefault()}
              ondrop={() => dropIntoSection(section as MetricSection)}
            >
              Drag metrics here
            </div>
          {/if}
          {#each sectionMetrics as metric (metric.id)}
            <div
              role="listitem"
              class:disabled={!metric.enabled}
              class:dragging={dragged === metric.id}
              class="customize-metric-row"
              draggable="true"
              ondragstart={(event) => {
                dragged = metric.id;
                beginDrag(
                  event,
                  metricDefinition(metric.id)?.label ?? metric.id,
                  section === 'alwaysVisible' ? 'Always Visible' : 'On Demand',
                );
              }}
              ondragend={() => (dragged = null)}
              ondragover={(event) => event.preventDefault()}
              ondrop={() => drop(metric, section as MetricSection)}
            >
              <span class="reorder-grip" aria-hidden="true"
                ><Icon name="grip-lines" size={16} strokeWidth={2} /></span
              >
              <span class="customize-metric-name"
                >{metricDefinition(metric.id)?.label ?? metric.id}</span
              >
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
