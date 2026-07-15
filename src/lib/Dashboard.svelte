<script lang="ts">
  import { onDestroy } from 'svelte';
  import { flip } from 'svelte/animate';
  import { scale, slide } from 'svelte/transition';
  import { reorderFlip, springMotion } from './motion';
  import { pointerReorder } from './pointerReorder';
  import ProviderIcon from './ProviderIcon.svelte';
  import ProviderNoticeRow from './ProviderNoticeRow.svelte';
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
    UpdateFailure,
    UsageHistory,
    UsageViewState,
    UpdateStatus,
  } from './types';

  interface Props {
    viewState: UsageViewState;
    settings: AppSettings;
    now: number;
    onSettingsChange: (settings: AppSettings) => void;
    onCustomizationChange: (settings: AppSettings) => void;
    onReorderStart: () => void;
    onReorderEnd: (moved: boolean, cancelled?: boolean) => void;
    onCustomize: () => void;
    onOpenProviderCustomize: (providerId: string) => void;
    onShare: (providerId: string) => void;
    onShareTotal: (projection: SpendProjection) => boolean | Promise<boolean>;
    onRefresh: (providerId: string) => void;
    onContentMorph: () => void;
    reducedMotion: boolean;
    updateStatus: UpdateStatus | null;
    installingUpdate: boolean;
    updateProgress: UpdateProgress | null;
    updateError: UpdateFailure | null;
    onInstallUpdate: () => void;
    onOpenUpdatePage: () => void;
  }
  let {
    viewState,
    settings,
    now,
    onSettingsChange,
    onCustomizationChange,
    onReorderStart,
    onReorderEnd,
    onCustomize,
    onOpenProviderCustomize,
    onShare,
    onShareTotal,
    onRefresh,
    onContentMorph,
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
  let providerMenu = $state<{ id: string; x: number; y: number } | null>(null);
  let metricMenu = $state<{ providerId: string; metricId: string; x: number; y: number } | null>(
    null,
  );
  let demandMorphing = $state(false);
  let demandMorphTimer: ReturnType<typeof setTimeout> | undefined;
  const enabledProviders = $derived(settings.providers.filter((provider) => provider.enabled));
  const dashboardProviders = $derived(
    enabledProviders.map((provider) => ({
      provider,
      state: viewState.providers[provider.id],
      alwaysMetrics: provider.metrics.filter(
        (metric) => metric.enabled && metric.section === 'alwaysVisible',
      ),
      demandMetrics: provider.metrics.filter(
        (metric) => metric.enabled && metric.section === 'onDemand',
      ),
    })),
  );
  const providerUsage = $derived(
    enabledProviders
      .filter((provider) => providerSupportsSpend(provider.id))
      .map((provider) => ({
        id: provider.id,
        usage: viewState.providers[provider.id]?.snapshot?.usage ?? emptyUsage,
      })),
  );

  function updateProvider(next: ProviderLayout, customization = true) {
    const changed = {
      ...settings,
      providers: settings.providers.map((item) => (item.id === next.id ? next : item)),
    };
    (customization ? onCustomizationChange : onSettingsChange)(changed);
  }
  function toggleDemandMetrics(provider: ProviderLayout) {
    window.clearTimeout(demandMorphTimer);
    demandMorphing = !reducedMotion;
    if (demandMorphing) {
      demandMorphTimer = window.setTimeout(
        () => (demandMorphing = false),
        springMotion(false).duration + 34,
      );
    }
    onContentMorph();
    updateProvider({ ...provider, expanded: !provider.expanded }, false);
  }
  onDestroy(() => window.clearTimeout(demandMorphTimer));
  function reorderProvider(draggedId: string, targetId: string) {
    if (draggedId === targetId) return;
    const enabled = settings.providers.filter((provider) => provider.enabled);
    const from = enabled.findIndex((provider) => provider.id === draggedId);
    const to = enabled.findIndex((provider) => provider.id === targetId);
    if (from < 0 || to < 0) return;
    const [moved] = enabled.splice(from, 1);
    enabled.splice(to, 0, moved);
    const providers = [...enabled, ...settings.providers.filter((provider) => !provider.enabled)];
    onCustomizationChange({ ...settings, providers });
  }
  function reorderMetric(
    draggedMetricId: string,
    providerId: string,
    targetMetricId: string,
    targetSection: MetricLayout['section'],
  ) {
    const provider = settings.providers.find((item) => item.id === providerId);
    if (!provider || draggedMetricId === targetMetricId) return;
    const metrics = [...provider.metrics];
    const from = metrics.findIndex((metric) => metric.id === draggedMetricId);
    const to = metrics.findIndex((metric) => metric.id === targetMetricId);
    if (from < 0 || to < 0) return;
    const [source] = metrics.splice(from, 1);
    const moved = { ...source, section: targetSection };
    metrics.splice(to, 0, moved);
    updateProvider({ ...provider, metrics });
  }
  function reorderMetricToTarget(
    draggedMetricId: string,
    providerId: string,
    targetMetricId: string,
  ) {
    if (targetMetricId === 'section:onDemand') {
      moveMetricIntoSection(draggedMetricId, providerId, 'onDemand');
      return;
    }
    const target = settings.providers
      .find((provider) => provider.id === providerId)
      ?.metrics.find((metric) => metric.id === targetMetricId);
    if (target) reorderMetric(draggedMetricId, providerId, targetMetricId, target.section);
  }
  function moveMetricIntoSection(
    draggedMetricId: string,
    providerId: string,
    section: MetricLayout['section'],
  ) {
    const provider = settings.providers.find((item) => item.id === providerId);
    if (!provider) return;
    const metrics = [...provider.metrics];
    const from = metrics.findIndex((metric) => metric.id === draggedMetricId);
    if (from < 0) return;
    const [source] = metrics.splice(from, 1);
    const lastInSection = metrics.reduce(
      (last, metric, index) => (metric.section === section ? index : last),
      -1,
    );
    const insertAt =
      lastInSection >= 0 ? lastInSection + 1 : section === 'alwaysVisible' ? 0 : metrics.length;
    metrics.splice(insertAt, 0, { ...source, section });
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
      {#if updateStatus.body}<details class="update-notes">
          <summary>What’s new</summary>
          <p>{updateStatus.body}</p>
        </details>{/if}
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
            : updateProgress.phase === 'retrying'
              ? 'Download interrupted. Retrying…'
              : updateProgress.percent === null
                ? 'Downloading update…'
                : `Downloading update… ${updateProgress.percent}%`}
        </small>
      {/if}
      {#if updateError}<div class="update-error" role="alert">
          <strong>{updateError.message}</strong><small>{updateError.action}</small>
        </div>{/if}
    </div>
    <div class="update-actions">
      <button
        type="button"
        class="update-primary-action"
        onclick={updateStatus.installable ? onInstallUpdate : onOpenUpdatePage}
        disabled={installingUpdate}
        >{updateStatus.installable
          ? installingUpdate
            ? 'Updating…'
            : updateError?.retryable
              ? 'Try Again'
              : 'Install Update'
          : 'Download from GitHub'}</button
      >
      {#if updateStatus.installable && !installingUpdate}
        <button type="button" class="update-release-action" onclick={onOpenUpdatePage}
          >View Release</button
        >
      {/if}
    </div>
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
  <section class="detection-card" out:scale={{ start: 0.95, ...springMotion(reducedMotion) }}>
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

{#each dashboardProviders as { provider, state, alwaysMetrics, demandMetrics } (provider.id)}
  <div
    class="provider-reorder-shell"
    class:provider-reorder-shell--content-morph={demandMorphing}
    animate:flip={reorderFlip(reducedMotion || demandMorphing)}
  >
    {#if state?.snapshot}
      <section
        class="provider-section"
        data-provider-id={provider.id}
        data-reorder-group="dashboard-providers"
        data-reorder-id={provider.id}
        role="group"
        aria-label={`${providerDisplayName(provider.id)} provider`}
        use:pointerReorder={{
          id: provider.id,
          group: 'dashboard-providers',
          label: providerDisplayName(provider.id),
          gripOnly: true,
          touchGripOnly: true,
          onReorder: (targetId) => reorderProvider(provider.id, targetId),
          onStart: onReorderStart,
          onEnd: onReorderEnd,
        }}
        oncontextmenu={(event) => openProviderMenu(event, provider.id)}
      >
        <header
          class="provider-header"
          data-reorder-handle
          role="group"
          aria-label={`Drag ${providerDisplayName(provider.id)} to reorder`}
        >
          <span
            class="drag-grip"
            data-reorder-handle
            data-reorder-touch-handle
            role="button"
            tabindex="0"
            aria-label={`Move ${providerDisplayName(provider.id)}`}
            aria-describedby="reorder-instructions"
            aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"><Icon name="grip-dots" size={13} /></span
          >
          <h1>{providerDisplayName(provider.id)}</h1>
          {#if state.snapshot.plan}<span class="plan">{state.snapshot.plan}</span>{/if}
          {#if state.stale}<span
              class="status-badge"
              data-tooltip={stalenessTooltip(state.snapshot.refreshedAt)}>Outdated</span
            >{/if}
          <span
            class="provider-status-slot"
            class:active={state.refreshing ||
              state.error !== null ||
              state.snapshot.warnings.length > 0}
          >
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
            {:else if state.snapshot.warnings.length > 0}
              <span
                class="provider-warning"
                role="status"
                data-tooltip={state.snapshot.warnings.join('\n')}
                aria-label={state.snapshot.warnings.join(' ')}
                ><Icon name="warning" size={12} strokeWidth={2} /><span class="sr-only"
                  >{state.snapshot.warnings.join(' ')}</span
                ></span
              >
            {/if}
          </span>
          <span class="provider-mark"><ProviderIcon providerId={provider.id} size={17} /></span>
        </header>
        <section class="provider-card" aria-label={`${providerDisplayName(provider.id)} usage`}>
          {#each state.snapshot.notices as notice (notice.id)}
            <ProviderNoticeRow {notice} />
          {/each}
          {#each alwaysMetrics as metric (metric.id)}
            <div
              class="metric-context-target"
              class:metric-context-target--content-morph={demandMorphing}
              data-reorder-group={`dashboard-metrics:${provider.id}`}
              data-reorder-id={metric.id}
              role="group"
              aria-label={`${metricDefinition(metric.id)?.label ?? metric.id} options`}
              use:pointerReorder={{
                id: metric.id,
                group: `dashboard-metrics:${provider.id}`,
                label: metricDefinition(metric.id)?.label ?? metric.id,
                touchGripOnly: true,
                onReorder: (targetId) => reorderMetricToTarget(metric.id, provider.id, targetId),
                onStart: onReorderStart,
                onEnd: onReorderEnd,
              }}
              animate:flip={reorderFlip(reducedMotion || demandMorphing)}
              oncontextmenu={(event) => openMetricMenu(event, provider.id, metric.id)}
            >
              <button
                class="metric-reorder-handle"
                data-reorder-handle
                data-reorder-touch-handle
                type="button"
                aria-label={`Move ${metricDefinition(metric.id)?.label ?? metric.id}`}
                aria-describedby="reorder-instructions"
                aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
                ><Icon name="grip-lines" size={13} strokeWidth={2} /></button
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
              data-reorder-group={`dashboard-metrics:${provider.id}`}
              data-reorder-id="section:onDemand"
              type="button"
              aria-expanded={provider.expanded}
              aria-label={provider.expanded ? 'Show less' : 'Show more'}
              onclick={() => toggleDemandMetrics(provider)}
            >
              <Icon
                name={provider.expanded ? 'chevron-up' : 'chevron-down'}
                size={10}
                strokeWidth={2.2}
              />
            </button>
            {#if provider.expanded}
              <div class="demand-metrics" transition:slide={springMotion(reducedMotion)}>
                {#each demandMetrics as metric (metric.id)}
                  <div
                    class="metric-context-target"
                    class:metric-context-target--content-morph={demandMorphing}
                    data-reorder-group={`dashboard-metrics:${provider.id}`}
                    data-reorder-id={metric.id}
                    role="group"
                    aria-label={`${metricDefinition(metric.id)?.label ?? metric.id} options`}
                    use:pointerReorder={{
                      id: metric.id,
                      group: `dashboard-metrics:${provider.id}`,
                      label: metricDefinition(metric.id)?.label ?? metric.id,
                      touchGripOnly: true,
                      onReorder: (targetId) =>
                        reorderMetricToTarget(metric.id, provider.id, targetId),
                      onStart: onReorderStart,
                      onEnd: onReorderEnd,
                    }}
                    animate:flip={reorderFlip(reducedMotion || demandMorphing)}
                    oncontextmenu={(event) => openMetricMenu(event, provider.id, metric.id)}
                  >
                    <button
                      class="metric-reorder-handle"
                      data-reorder-handle
                      data-reorder-touch-handle
                      type="button"
                      aria-label={`Move ${metricDefinition(metric.id)?.label ?? metric.id}`}
                      aria-describedby="reorder-instructions"
                      aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
                      ><Icon name="grip-lines" size={13} strokeWidth={2} /></button
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
      </section>
    {:else if state?.error}
      <section
        class="provider-section"
        data-provider-id={provider.id}
        data-reorder-group="dashboard-providers"
        data-reorder-id={provider.id}
        role="group"
        aria-label={`${providerDisplayName(provider.id)} provider`}
        use:pointerReorder={{
          id: provider.id,
          group: 'dashboard-providers',
          label: providerDisplayName(provider.id),
          gripOnly: true,
          touchGripOnly: true,
          onReorder: (targetId) => reorderProvider(provider.id, targetId),
          onStart: onReorderStart,
          onEnd: onReorderEnd,
        }}
        oncontextmenu={(event) => openProviderMenu(event, provider.id)}
      >
        <header
          class="provider-header"
          data-reorder-handle
          role="group"
          aria-label={`Drag ${providerDisplayName(provider.id)} to reorder`}
        >
          <span
            class="drag-grip"
            data-reorder-handle
            data-reorder-touch-handle
            role="button"
            tabindex="0"
            aria-label={`Move ${providerDisplayName(provider.id)}`}
            aria-describedby="reorder-instructions"
            aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"><Icon name="grip-dots" size={13} /></span
          >
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
        data-reorder-group="dashboard-providers"
        data-reorder-id={provider.id}
        role="group"
        aria-label={`${providerDisplayName(provider.id)} provider`}
        use:pointerReorder={{
          id: provider.id,
          group: 'dashboard-providers',
          label: providerDisplayName(provider.id),
          gripOnly: true,
          touchGripOnly: true,
          onReorder: (targetId) => reorderProvider(provider.id, targetId),
          onStart: onReorderStart,
          onEnd: onReorderEnd,
        }}
      >
        <header class="provider-header" data-reorder-handle>
          <span
            class="drag-grip"
            data-reorder-handle
            data-reorder-touch-handle
            role="button"
            tabindex="0"
            aria-label={`Move ${providerDisplayName(provider.id)}`}
            aria-describedby="reorder-instructions"
            aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"><Icon name="grip-dots" size={13} /></span
          >
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
        data-reorder-group="dashboard-providers"
        data-reorder-id={provider.id}
        role="group"
        aria-label={`${providerDisplayName(provider.id)} provider`}
        use:pointerReorder={{
          id: provider.id,
          group: 'dashboard-providers',
          label: providerDisplayName(provider.id),
          gripOnly: true,
          touchGripOnly: true,
          onReorder: (targetId) => reorderProvider(provider.id, targetId),
          onStart: onReorderStart,
          onEnd: onReorderEnd,
        }}
      >
        <header class="provider-header" data-reorder-handle>
          <span
            class="drag-grip"
            data-reorder-handle
            data-reorder-touch-handle
            role="button"
            tabindex="0"
            aria-label={`Move ${providerDisplayName(provider.id)}`}
            aria-describedby="reorder-instructions"
            aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"><Icon name="grip-dots" size={13} /></span
          >
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
  </div>
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
      <button type="button" role="menuitem" onclick={() => onRefresh(menuProvider.id)}
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
      <button type="button" role="menuitem" onclick={() => onRefresh(metricProvider.id)}
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

<style>
  :global {
    .provider-header {
      display: flex;
      min-height: 28px;
      align-items: center;
      gap: 7px;
      padding: 0 4px 4px 2px;
    }

    .provider-header h1 {
      margin: 0;
      font-size: 14px;
      font-weight: 650;
      letter-spacing: -0.01em;
    }

    .plan {
      color: var(--secondary);
      font-size: 11px;
      line-height: 1;
      white-space: nowrap;
    }

    .status-badge {
      padding: 2px 6px;
      border-radius: 999px;
      color: var(--warning);
      background: var(--warning-bg);
      font-size: 9px;
      font-weight: 650;
      text-transform: uppercase;
      letter-spacing: 0.04em;
    }

    .provider-mark {
      display: grid;
      margin-left: auto;
      color: var(--secondary);
      place-items: center;
    }

    .provider-status-slot {
      display: grid;
      width: 16px;
      height: 18px;
      flex: 0 0 16px;
      margin-left: auto;
      opacity: 0;
      place-items: center;
    }

    .provider-status-slot.active {
      opacity: 1;
    }

    .provider-header .provider-mark {
      margin-left: 0;
    }

    .provider-warning,
    .provider-refreshing {
      display: grid;
      width: 16px;
      height: 18px;
      flex: 0 0 16px;
      place-items: center;
    }

    .provider-warning {
      color: var(--warning);
      cursor: help;
    }

    .provider-refreshing {
      color: var(--secondary);
      animation: spin 900ms linear infinite;
    }

    .drag-grip {
      display: grid;
      width: 8px;
      grid-template-columns: repeat(2, 2px);
      grid-template-rows: repeat(3, 2px);
      gap: 2px;
      opacity: 0.6;
    }

    .drag-grip i {
      width: 2px;
      height: 2px;
      border-radius: 50%;
      background: var(--tertiary);
    }

    .provider-card {
      padding: 5px 12px;
      border-radius: 12px;
      background: var(--card);
    }

    .demand-divider {
      display: grid;
      width: 100%;
      min-height: 24px;
      padding: 5px 14px;
      border: 0;
      color: var(--secondary);
      background: none;
      cursor: pointer;
      place-items: center;
    }

    .demand-metrics {
      overflow: hidden;
    }

    .provider-reorder-shell--content-morph {
      transform: none !important;
    }

    .metric-context-target--content-morph {
      transform: none !important;
    }

    .detection-card {
      position: relative;
      display: grid;
      grid-template-columns: 1fr auto;
      gap: 8px;
      margin-bottom: 10px;
      padding: 10px 28px 10px 11px;
      border-radius: 11px;
      background: color-mix(in srgb, var(--provider) 11%, var(--card));
    }

    .hint-card {
      position: relative;
      display: grid;
      grid-template-columns: auto 1fr;
      gap: 3px 10px;
      margin-bottom: 14px;
      padding: 11px 32px 11px 12px;
      border-radius: 12px;
      background: var(--card);
    }

    .hint-card__icon {
      display: grid;
      width: 22px;
      height: 22px;
      grid-row: 1 / span 2;
      color: var(--meter-fill);
      place-items: center;
    }

    .hint-card > div {
      display: flex;
      min-width: 0;
      flex-direction: column;
      gap: 2px;
    }

    .hint-card strong {
      font-size: 12px;
    }

    .hint-card span {
      color: var(--secondary);
      font-size: 10px;
      line-height: 13px;
    }

    .update-actions button {
      width: fit-content;
      padding: 4px 8px;
      border: 0;
      border-radius: 6px;
      color: var(--text);
      background: var(--button-hover);
      font-size: 10px;
    }

    .hint-card > .update-actions {
      display: flex;
      grid-column: 2;
      align-items: center;
      flex-direction: row;
      gap: 6px;
      margin-top: 5px;
    }

    .update-actions .update-primary-action {
      color: white;
      background: var(--meter-fill);
      font-weight: 600;
    }

    .update-actions .update-release-action {
      color: var(--secondary);
      background: transparent;
    }

    .update-actions .update-release-action:hover,
    .update-actions .update-release-action:focus-visible {
      color: var(--text);
      background: var(--button-hover);
    }

    .hint-card__dismiss {
      position: absolute;
      top: 5px;
      right: 6px;
      display: grid;
      width: 20px;
      height: 20px;
      padding: 0;
      border: 0;
      border-radius: 5px;
      color: var(--secondary);
      background: transparent;
      cursor: pointer;
      place-items: center;
    }

    .hint-card__dismiss:hover,
    .hint-card__dismiss:focus-visible,
    .detection-card .dismiss:hover,
    .detection-card .dismiss:focus-visible {
      outline: none;
      color: var(--text);
      background: var(--button-hover);
    }

    .update-notes {
      margin-top: 3px;
      color: var(--secondary);
      font-size: 9px;
      line-height: 12px;
    }

    .update-notes summary {
      width: fit-content;
      color: var(--secondary);
      cursor: pointer;
      font-size: 10px;
    }

    .update-notes p {
      max-height: 72px;
      margin: 5px 0 0;
      padding-right: 3px;
      overflow-y: auto;
      white-space: pre-line;
    }

    .update-error {
      gap: 1px;
      margin-top: 4px;
      padding: 6px 7px;
      border-radius: 7px;
      color: var(--error);
      background: var(--error-bg);
    }

    .update-error strong {
      font-size: 10px;
    }

    .update-error small {
      color: var(--error);
      font-size: 9px;
      line-height: 12px;
    }

    .update-progress {
      height: 4px;
      margin-top: 4px;
      overflow: hidden;
      border-radius: 999px;
      background: var(--meter-track);
    }

    .update-progress span {
      display: block;
      min-width: 8%;
      height: 100%;
      border-radius: inherit;
      background: var(--meter-fill);
      transition: width 160ms ease;
    }

    .detection-card div {
      display: flex;
      min-width: 0;
      flex-direction: column;
      gap: 2px;
    }

    .detection-card strong {
      font-size: 12px;
    }

    .detection-card span {
      color: var(--secondary);
      font-size: 10px;
      line-height: 14px;
    }

    .detection-card button {
      align-self: center;
      padding: 4px 8px;
      border: 0;
      border-radius: 6px;
      color: white;
      background: var(--provider);
      font-size: 10px;
      cursor: pointer;
    }

    .detection-card .dismiss {
      position: absolute;
      top: 4px;
      right: 4px;
      display: grid;
      width: 20px;
      height: 20px;
      padding: 0;
      border: 0;
      border-radius: 5px;
      color: var(--secondary);
      background: transparent;
      cursor: pointer;
      place-items: center;
    }

    .empty-dashboard {
      display: flex;
      min-height: 0;
      align-items: center;
      justify-content: center;
      padding: 24px 16px;
      color: var(--secondary);
      text-align: center;
    }

    .empty-dashboard strong {
      color: var(--text);
      font-size: 13px;
    }

    .empty-dashboard span {
      width: 100%;
      font-size: 12px;
    }

    .warning {
      margin: 9px 2px 0;
    }

    .provider-header {
      min-height: 24px;
      gap: 6px;
      margin-bottom: 4px;
      padding: 0 4px 2px 2px;
      cursor: grab;
    }

    .provider-header:active {
      cursor: grabbing;
    }

    .provider-header h1 {
      font-size: 14px;
      font-weight: 600;
      letter-spacing: 0;
    }

    .status-badge {
      padding: 0;
      color: var(--tertiary);
      background: transparent;
      font-size: 11px;
      font-weight: 400;
      text-transform: none;
      letter-spacing: 0;
    }

    .provider-mark {
      width: 16px;
      height: 16px;
      color: var(--text);
    }

    .drag-grip {
      width: 7px;
      grid-template-columns: repeat(2, 1.75px);
      grid-template-rows: repeat(3, 1.75px);
      gap: 2px;
      opacity: 1;
    }

    .drag-grip i {
      width: 1.75px;
      height: 1.75px;
    }

    .provider-card {
      padding: 5px 8px;
      border: 0;
      border-radius: 12px;
    }

    .detection-card {
      margin-bottom: 14px;
      padding: 10px 30px 10px 12px;
      border-radius: 12px;
      background: var(--card);
    }

    .detection-card button {
      color: var(--text);
      background: var(--button-hover);
    }

    .detection-card .dismiss {
      color: var(--secondary);
      background: transparent;
    }

    .metric-context-target {
      position: relative;
      cursor: grab;
    }

    .metric-context-target:active {
      cursor: grabbing;
    }

    .metric-reorder-handle {
      position: absolute;
      top: 50%;
      left: -7px;
      z-index: 2;
      display: grid;
      width: 18px;
      height: 24px;
      padding: 0;
      border: 0;
      border-radius: 8px;
      color: var(--tertiary);
      background: color-mix(in srgb, var(--card) 88%, transparent);
      opacity: 0;
      cursor: grab;
      transform: translateY(-50%);
      place-items: center;
      transition: opacity 140ms ease;
    }

    .metric-context-target:hover > .metric-reorder-handle {
      opacity: 0.82;
    }

    .context-menu {
      position: fixed;
      z-index: 80;
      width: 190px;
      padding: 4px;
      border: 1px solid var(--separator);
      border-radius: 10px;
      background: color-mix(in srgb, var(--tray) 97%, transparent);
      box-shadow: 0 12px 32px rgba(0, 0, 0, 0.3);
      backdrop-filter: blur(18px);
      animation: menu-in 180ms ease-out both;
    }

    .context-menu button {
      display: flex;
      width: 100%;
      align-items: center;
      gap: 8px;
      padding: 6px 8px;
      border: 0;
      border-radius: 6px;
      color: var(--text);
      background: transparent;
      font-size: 11px;
      text-align: left;
    }

    .context-menu button.danger {
      color: var(--meter-critical);
    }

    .context-menu button:disabled {
      opacity: 0.45;
    }

    @keyframes spin {
      to {
        transform: rotate(360deg);
      }
    }
  }
</style>
