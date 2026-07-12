<script lang="ts">
  import { scale, slide } from 'svelte/transition';
  import { beginDrag } from './dragPreview';
  import ProviderIcon from './ProviderIcon.svelte';
  import Icon from './Icon.svelte';
  import MetricRenderer from './MetricRenderer.svelte';
  import TotalSpend from './TotalSpend.svelte';
  import type { SpendProjection } from './totalSpend';
  import { metricDefinition, providerDisplayName, providerSupportsSpend } from './metrics';
  import type {
    AppSettings,
    MetricLayout,
    ProviderLayout,
    UpdateProgress,
    UsageHistory,
    UsageViewState,
    UpdateStatus,
  } from './types';

  interface Props {
    viewState: UsageViewState;
    settings: AppSettings;
    now: number;
    onSettingsChange: (settings: AppSettings) => void;
    onCustomize: () => void;
    onOpenProviderCustomize: (providerId: string) => void;
    onShare: (providerId: string) => void;
    onShareTotal: (projection: SpendProjection) => boolean | Promise<boolean>;
    onRefresh: () => void;
    reducedMotion: boolean;
    updateStatus: UpdateStatus | null;
    installingUpdate: boolean;
    updateProgress: UpdateProgress | null;
    updateError: string | null;
    onInstallUpdate: () => void;
    onOpenUpdatePage: () => void;
  }
  let {
    viewState,
    settings,
    now,
    onSettingsChange,
    onCustomize,
    onOpenProviderCustomize,
    onShare,
    onShareTotal,
    onRefresh,
    reducedMotion,
    updateStatus,
    installingUpdate,
    updateProgress,
    updateError,
    onInstallUpdate,
    onOpenUpdatePage,
  }: Props = $props();
  const emptyUsage: UsageHistory = {
    today: null,
    yesterday: null,
    last30Days: null,
    daily: [],
    unknownModels: [],
  };
  let draggedProvider = $state<string | null>(null);
  let draggedMetric = $state<{ providerId: string; metricId: string } | null>(null);
  let providerMenu = $state<{ id: string; x: number; y: number } | null>(null);
  let metricMenu = $state<{ providerId: string; metricId: string; x: number; y: number } | null>(
    null,
  );
  const enabledProviders = $derived(settings.providers.filter((provider) => provider.enabled));
  const providerUsage = $derived(
    enabledProviders
      .filter((provider) => providerSupportsSpend(provider.id))
      .map((provider) => ({
        id: provider.id,
        usage: viewState.providers[provider.id]?.snapshot?.usage ?? emptyUsage,
      })),
  );

  function updateProvider(next: ProviderLayout) {
    onSettingsChange({
      ...settings,
      providers: settings.providers.map((item) => (item.id === next.id ? next : item)),
    });
  }
  function reorderProvider(targetId: string) {
    if (!draggedProvider || draggedProvider === targetId) return;
    const providers = [...settings.providers];
    const from = providers.findIndex((provider) => provider.id === draggedProvider);
    const to = providers.findIndex((provider) => provider.id === targetId);
    if (from < 0 || to < 0) return;
    const [moved] = providers.splice(from, 1);
    providers.splice(to, 0, moved);
    draggedProvider = null;
    onSettingsChange({ ...settings, providers });
  }
  function reorderMetric(
    event: DragEvent,
    providerId: string,
    targetMetricId: string,
    targetSection: MetricLayout['section'],
  ) {
    event.preventDefault();
    event.stopPropagation();
    if (!draggedMetric || draggedMetric.providerId !== providerId) return;
    const provider = settings.providers.find((item) => item.id === providerId);
    if (!provider || draggedMetric.metricId === targetMetricId) return;
    const metrics = [...provider.metrics];
    const from = metrics.findIndex((metric) => metric.id === draggedMetric?.metricId);
    const to = metrics.findIndex((metric) => metric.id === targetMetricId);
    if (from < 0 || to < 0) return;
    const [moved] = metrics.splice(from, 1);
    moved.section = targetSection;
    metrics.splice(to, 0, moved);
    draggedMetric = null;
    updateProvider({ ...provider, metrics });
  }
  function openProviderMenu(event: MouseEvent, providerId: string) {
    event.preventDefault();
    metricMenu = null;
    providerMenu = {
      id: providerId,
      x: Math.max(6, Math.min(event.clientX, window.innerWidth - 196)),
      y: Math.max(6, Math.min(event.clientY, window.innerHeight - 174)),
    };
    queueMicrotask(focusFirstMenuItem);
  }
  function openMetricMenu(event: MouseEvent, providerId: string, metricId: string) {
    event.preventDefault();
    event.stopPropagation();
    providerMenu = null;
    metricMenu = {
      providerId,
      metricId,
      x: Math.max(6, Math.min(event.clientX, window.innerWidth - 196)),
      y: Math.max(6, Math.min(event.clientY, window.innerHeight - 154)),
    };
    queueMicrotask(focusFirstMenuItem);
  }
  function focusFirstMenuItem() {
    document.querySelector<HTMLButtonElement>('.context-menu button:not(:disabled)')?.focus();
  }
  function handleContextMenuKey(event: KeyboardEvent) {
    const menu = event.currentTarget as HTMLElement;
    const items = [...menu.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      providerMenu = null;
      metricMenu = null;
      return;
    }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) || items.length === 0) return;
    event.preventDefault();
    const current = Math.max(0, items.indexOf(document.activeElement as HTMLButtonElement));
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? items.length - 1
          : event.key === 'ArrowDown'
            ? (current + 1) % items.length
            : (current - 1 + items.length) % items.length;
    items[next].focus();
  }
  function patchMetric(providerId: string, metricId: string, patch: Partial<MetricLayout>) {
    const provider = settings.providers.find((item) => item.id === providerId);
    if (!provider) return;
    updateProvider({
      ...provider,
      metrics: provider.metrics.map((metric) =>
        metric.id === metricId ? { ...metric, ...patch } : metric,
      ),
    });
    metricMenu = null;
  }
  function hideProvider(providerId: string) {
    const provider = settings.providers.find((item) => item.id === providerId);
    if (provider) updateProvider({ ...provider, enabled: false });
    providerMenu = null;
  }
  function dismissDetection() {
    onSettingsChange({ ...settings, detectionNoticeDismissed: true });
  }
  function springOut(progress: number) {
    return 1 - Math.pow(1 - progress, 3);
  }
  function stalenessTooltip(refreshedAt: string) {
    const elapsedSeconds = Math.max(0, Math.floor((now - Date.parse(refreshedAt)) / 1000));
    if (!Number.isFinite(elapsedSeconds)) return 'Last update time unavailable';
    if (elapsedSeconds < 60) return 'Last updated moments ago';
    const minutes = Math.floor(elapsedSeconds / 60);
    if (minutes < 60) return `Last updated ${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    const remainingMinutes = minutes % 60;
    return `Last updated ${hours}h${remainingMinutes ? ` ${remainingMinutes}m` : ''} ago`;
  }
</script>

<svelte:window
  onclick={() => {
    providerMenu = null;
    metricMenu = null;
  }}
/>

{#if updateStatus?.available && updateStatus.version !== settings.dismissedUpdateVersion}
  <section class="hint-card update-banner" aria-label="Update Available">
    <span class="hint-card__icon"><Icon name="refresh" size={16} strokeWidth={2} /></span>
    <div>
      <strong>Update Available</strong>
      <span>OpenQuota {updateStatus.version} is ready to download.</span>
      {#if updateStatus.body}<small class="update-notes">{updateStatus.body}</small>{/if}
      {#if installingUpdate && updateProgress}
        <div
          class="update-progress"
          role="progressbar"
          aria-label="Update download"
          aria-valuemin="0"
          aria-valuemax="100"
          aria-valuenow={updateProgress.phase === 'installing'
            ? 100
            : (updateProgress.percent ?? undefined)}
        >
          <span
            style:width={`${updateProgress.phase === 'installing' ? 100 : (updateProgress.percent ?? 8)}%`}
          ></span>
        </div>
        <small>
          {updateProgress.phase === 'installing'
            ? 'Installing update…'
            : updateProgress.percent === null
              ? 'Downloading update…'
              : `Downloading update… ${updateProgress.percent}%`}
        </small>
      {/if}
      {#if updateError}<small class="notice-text">{updateError}</small>{/if}
    </div>
    <button
      type="button"
      onclick={updateStatus.installable ? onInstallUpdate : onOpenUpdatePage}
      disabled={installingUpdate}
      >{updateStatus.installable
        ? installingUpdate
          ? 'Updating…'
          : 'Install Update'
        : 'Download Update'}</button
    >
    <button
      class="hint-card__dismiss"
      type="button"
      aria-label="Dismiss"
      onclick={() =>
        onSettingsChange({
          ...settings,
          dismissedUpdateVersion: updateStatus?.version ?? null,
        })}><Icon name="close" size={10} strokeWidth={2.2} /></button
    >
  </section>
{/if}

{#if !settings.detectionNoticeDismissed}
  <section
    class="detection-card"
    out:scale={{ start: 0.95, duration: reducedMotion ? 0 : 420, easing: springOut }}
  >
    <div>
      <strong>Welcome to OpenQuota</strong><span
        >We set you up with the AI tools found on your computer. Add or hide providers any time.</span
      >
    </div>
    <button type="button" onclick={onCustomize}>Open Customize</button>
    <button class="dismiss" type="button" aria-label="Dismiss" onclick={dismissDetection}
      ><Icon name="close" size={10} strokeWidth={2.2} /></button
    >
  </section>
{/if}

{#if settings.showTotalSpend && providerUsage.length > 0}
  <TotalSpend
    providers={providerUsage}
    {settings}
    onChange={onSettingsChange}
    onShare={onShareTotal}
  />
{/if}

{#each enabledProviders as provider (provider.id)}
  {@const state = viewState.providers[provider.id]}
  {@const alwaysMetrics = provider.metrics.filter(
    (metric) => metric.enabled && metric.section === 'alwaysVisible',
  )}
  {@const demandMetrics = provider.metrics.filter(
    (metric) => metric.enabled && metric.section === 'onDemand',
  )}
  {#if state?.snapshot}
    <section
      class="provider-section"
      class:dragging={draggedProvider === provider.id}
      data-provider-id={provider.id}
      role="group"
      aria-label={`${providerDisplayName(provider.id)} provider`}
      ondragover={(event) => event.preventDefault()}
      ondrop={() => reorderProvider(provider.id)}
      oncontextmenu={(event) => openProviderMenu(event, provider.id)}
    >
      <header
        class="provider-header"
        role="group"
        aria-label={`Drag ${providerDisplayName(provider.id)} to reorder`}
        draggable="true"
        ondragstart={(event) => {
          draggedProvider = provider.id;
          beginDrag(event, providerDisplayName(provider.id), state.snapshot?.plan ?? 'Provider');
        }}
        ondragend={() => (draggedProvider = null)}
      >
        <span class="drag-grip" aria-hidden="true"><Icon name="grip-dots" size={13} /></span>
        <h1>{providerDisplayName(provider.id)}</h1>
        {#if state.snapshot.plan}<span class="plan">{state.snapshot.plan}</span>{/if}
        {#if state.stale}<span
            class="status-badge"
            data-tooltip={stalenessTooltip(state.snapshot.refreshedAt)}>Outdated</span
          >{/if}
        <span class="provider-status-slot" class:active={state.refreshing || state.error !== null}>
          {#if state.refreshing}
            <span class="provider-refreshing" aria-label="Refreshing"
              ><Icon name="refresh" size={12} strokeWidth={2} /></span
            >
          {:else if state.error}
            <span
              class="provider-warning"
              role="alert"
              data-tooltip={state.error}
              aria-label={state.error}
              ><Icon name="warning" size={12} strokeWidth={2} /><span class="sr-only"
                >{state.error}</span
              ></span
            >
          {/if}
        </span>
        <span class="provider-mark"><ProviderIcon providerId={provider.id} size={17} /></span>
      </header>
      <section class="provider-card" aria-label={`${providerDisplayName(provider.id)} usage`}>
        {#each alwaysMetrics as metric (metric.id)}
          <div
            class="metric-context-target"
            class:dragging={draggedMetric?.metricId === metric.id}
            role="group"
            aria-label={`${metricDefinition(metric.id)?.label ?? metric.id} options`}
            draggable="true"
            ondragstart={(event) => {
              event.stopPropagation();
              draggedMetric = { providerId: provider.id, metricId: metric.id };
              beginDrag(event, metricDefinition(metric.id)?.label ?? metric.id, 'Always Visible');
            }}
            ondragend={() => (draggedMetric = null)}
            ondragover={(event) => {
              event.preventDefault();
              event.stopPropagation();
            }}
            ondrop={(event) => reorderMetric(event, provider.id, metric.id, 'alwaysVisible')}
            oncontextmenu={(event) => openMetricMenu(event, provider.id, metric.id)}
          >
            <MetricRenderer
              layout={metric}
              snapshot={state.snapshot}
              {settings}
              {now}
              {onSettingsChange}
            />
          </div>
        {/each}
        {#if demandMetrics.length > 0}
          <button
            class="demand-divider"
            type="button"
            aria-expanded={provider.expanded}
            aria-label={provider.expanded ? 'Show less' : 'Show more'}
            onclick={() => updateProvider({ ...provider, expanded: !provider.expanded })}
          >
            <Icon
              name={provider.expanded ? 'chevron-up' : 'chevron-down'}
              size={10}
              strokeWidth={2.2}
            />
          </button>
          {#if provider.expanded}
            <div
              class="demand-metrics"
              transition:slide={{ duration: reducedMotion ? 0 : 420, easing: springOut }}
            >
              {#each demandMetrics as metric (metric.id)}
                <div
                  class="metric-context-target"
                  class:dragging={draggedMetric?.metricId === metric.id}
                  role="group"
                  aria-label={`${metricDefinition(metric.id)?.label ?? metric.id} options`}
                  draggable="true"
                  ondragstart={(event) => {
                    event.stopPropagation();
                    draggedMetric = { providerId: provider.id, metricId: metric.id };
                    beginDrag(event, metricDefinition(metric.id)?.label ?? metric.id, 'On Demand');
                  }}
                  ondragend={() => (draggedMetric = null)}
                  ondragover={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                  }}
                  ondrop={(event) => reorderMetric(event, provider.id, metric.id, 'onDemand')}
                  oncontextmenu={(event) => openMetricMenu(event, provider.id, metric.id)}
                >
                  <MetricRenderer
                    layout={metric}
                    snapshot={state.snapshot}
                    {settings}
                    {now}
                    {onSettingsChange}
                  />
                </div>
              {/each}
            </div>
          {/if}
        {/if}
      </section>
      {#each state.snapshot.warnings as warning (warning)}<p class="warning">{warning}</p>{/each}
    </section>
  {:else if state?.error}
    <section
      class="provider-section"
      data-provider-id={provider.id}
      role="group"
      aria-label={`${providerDisplayName(provider.id)} provider`}
      ondragover={(event) => event.preventDefault()}
      ondrop={() => reorderProvider(provider.id)}
      oncontextmenu={(event) => openProviderMenu(event, provider.id)}
    >
      <header
        class="provider-header"
        role="group"
        aria-label={`Drag ${providerDisplayName(provider.id)} to reorder`}
        draggable="true"
        ondragstart={(event) => {
          draggedProvider = provider.id;
          beginDrag(event, providerDisplayName(provider.id), 'No usage data');
        }}
        ondragend={() => (draggedProvider = null)}
      >
        <span class="drag-grip" aria-hidden="true"><Icon name="grip-dots" size={13} /></span>
        <h1>{providerDisplayName(provider.id)}</h1>
        <span class="provider-status-slot active">
          <span
            class="provider-warning"
            role="alert"
            data-tooltip={state.error}
            aria-label={state.error}
            ><Icon name="warning" size={12} strokeWidth={2} /><span class="sr-only"
              >{state.error}</span
            ></span
          >
        </span>
        <span class="provider-mark"><ProviderIcon providerId={provider.id} size={17} /></span>
      </header>
      <section class="provider-card"><p class="empty-row">No usage data</p></section>
    </section>
  {:else if state?.refreshing}
    <section
      class="provider-section provider-section--pending"
      data-provider-id={provider.id}
      role="group"
      aria-label={`${providerDisplayName(provider.id)} provider`}
    >
      <header class="provider-header">
        <span class="drag-grip" aria-hidden="true"><Icon name="grip-dots" size={13} /></span>
        <h1>{providerDisplayName(provider.id)}</h1>
        <span class="provider-status-slot active">
          <span class="provider-refreshing" aria-label="Refreshing"
            ><Icon name="refresh" size={12} strokeWidth={2} /></span
          >
        </span>
        <span class="provider-mark"><ProviderIcon providerId={provider.id} size={17} /></span>
      </header>
      <section
        class="provider-card"
        aria-label={`${providerDisplayName(provider.id)} usage`}
        aria-busy="true"
      >
        <p class="empty-row">Reading {providerDisplayName(provider.id)} usage…</p>
      </section>
    </section>
  {:else if !state?.error}
    <section
      class="provider-section provider-section--pending"
      data-provider-id={provider.id}
      role="group"
      aria-label={`${providerDisplayName(provider.id)} provider`}
    >
      <header class="provider-header">
        <span class="drag-grip" aria-hidden="true"><Icon name="grip-dots" size={13} /></span>
        <h1>{providerDisplayName(provider.id)}</h1>
        <span class="provider-status-slot"></span>
        <span class="provider-mark"><ProviderIcon providerId={provider.id} size={17} /></span>
      </header>
      <section
        class="provider-card provider-card--pending"
        aria-label={`${providerDisplayName(provider.id)} usage`}
      >
        <p class="provider-pending-copy">No {providerDisplayName(provider.id)} data yet.</p>
      </section>
    </section>
  {/if}
{/each}

{#if providerMenu}
  {@const menuProvider = settings.providers.find((provider) => provider.id === providerMenu?.id)}
  {#if menuProvider}
    <div
      class="context-menu"
      style={`left:${providerMenu.x}px;top:${providerMenu.y}px`}
      role="menu"
      tabindex="-1"
      onkeydown={handleContextMenuKey}
    >
      <button
        class="danger"
        type="button"
        role="menuitem"
        onclick={() => hideProvider(menuProvider.id)}
        ><Icon name="power" size={15} />Hide {providerDisplayName(menuProvider.id)}</button
      >
      <hr />
      <button type="button" role="menuitem" onclick={onRefresh}
        ><Icon name="refresh" size={15} />Refresh {providerDisplayName(menuProvider.id)}</button
      >
      <button type="button" role="menuitem" onclick={() => onOpenProviderCustomize(menuProvider.id)}
        ><Icon name="sliders" size={15} />Customize…</button
      >
      <hr />
      <button type="button" role="menuitem" onclick={() => onShare(menuProvider.id)}
        ><Icon name="share" size={15} />Share Screenshot</button
      >
    </div>
  {/if}
{/if}

{#if metricMenu}
  {@const metricProvider = settings.providers.find(
    (provider) => provider.id === metricMenu?.providerId,
  )}
  {@const menuMetric = metricProvider?.metrics.find((metric) => metric.id === metricMenu?.metricId)}
  {#if metricProvider && menuMetric}
    <div
      class="context-menu"
      style={`left:${metricMenu.x}px;top:${metricMenu.y}px`}
      role="menu"
      tabindex="-1"
      onkeydown={handleContextMenuKey}
    >
      <button
        class="danger"
        type="button"
        role="menuitem"
        onclick={() =>
          patchMetric(metricProvider.id, menuMetric.id, { enabled: false, pinned: false })}
        ><Icon name="power" size={15} />Hide</button
      >
      {#if metricDefinition(menuMetric.id)?.pinnable}
        <button
          type="button"
          role="menuitem"
          disabled={!menuMetric.pinned &&
            metricProvider.metrics.filter((metric) => metric.pinned).length >= 2}
          onclick={() =>
            patchMetric(metricProvider.id, menuMetric.id, {
              pinned: !menuMetric.pinned,
              enabled: true,
            })}
          ><Icon name={menuMetric.pinned ? 'star-filled' : 'star'} size={15} />{menuMetric.pinned
            ? 'Unstar'
            : 'Star for menu bar'}</button
        >
      {/if}
      <hr />
      <button type="button" role="menuitem" onclick={onRefresh}
        ><Icon name="refresh" size={15} />Refresh {providerDisplayName(metricProvider.id)}</button
      >
      <button
        type="button"
        role="menuitem"
        onclick={() => onOpenProviderCustomize(metricProvider.id)}
        ><Icon name="sliders" size={15} />Customize…</button
      >
    </div>
  {/if}
{/if}

{#if enabledProviders.length === 0}
  <section class="empty-dashboard">
    <span>Turn on Customize to choose what to show.</span>
  </section>
{/if}
