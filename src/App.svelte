<script lang="ts">
  import { onMount } from 'svelte';
  import {
    dismissMainWindow,
    getBootstrapState,
    getLogPath,
    onOpenScreen,
    onPopupHidden,
    onSettingsState,
    onUpdateProgress,
    onUsageState,
    openProviderLink as openProviderLinkCommand,
    openNotificationSettings as openSystemNotificationSettings,
    openLogFolder as openSystemLogFolder,
    quitApplication,
    refreshProviderUsage,
    refreshUsage,
    requestNotificationPermission,
    resetCustomization as resetCustomizationCommand,
    resetProviderCustomization as resetProviderCustomizationCommand,
  } from './lib/backend';
  import CustomizeProviderDetail from './lib/CustomizeProviderDetail.svelte';
  import CustomizeProviderList from './lib/CustomizeProviderList.svelte';
  import ConfirmationSheet from './lib/ConfirmationSheet.svelte';
  import Dashboard from './lib/Dashboard.svelte';
  import Icon from './lib/Icon.svelte';
  import { createListenerRegistry } from './lib/listenerRegistry';
  import { emptyProviderCatalog, ProviderCatalogIndex } from './lib/metrics';
  import { springMotion } from './lib/motion';
  import OpenQuotaMark from './lib/OpenQuotaMark.svelte';
  import { horizontalPageTransition, shouldSlideBetweenScreens } from './lib/pageTransition';
  import { desktopPlatform, shortcutLabels } from './lib/platform';
  import {
    buildProviderShareRows,
    renderProviderShareCard,
    renderTotalSpendShareCard,
  } from './lib/shareCard';
  import SettingsScreen from './lib/SettingsScreen.svelte';
  import { SettingsController } from './lib/settingsController.svelte';
  import type { SpendProjection } from './lib/totalSpend';
  import type { AppSettings, UsageViewState } from './lib/types';
  import { nextUpdateLabel, UpdateController } from './lib/updateController.svelte';
  import { automaticUpdateDelay, UPDATE_CHECK_INTERVAL_MS } from './lib/updateSchedule';
  import { createWindowController, type AppScreen } from './lib/windowController';

  type Screen = AppScreen;
  const appVersion = import.meta.env.APP_VERSION;
  const emptyView: UsageViewState = { providers: {} };

  let viewState = $state<UsageViewState>(emptyView);
  let catalog = $state<ProviderCatalogIndex>(emptyProviderCatalog);
  let screen = $state<Screen>('dashboard');
  let now = $state(Date.now());
  let settingsError = $state<string | null>(null);
  let automaticUpdatesReady = $state(false);
  let reducedMotion = $state(false);
  let slideDirection = $state(1);
  let slidePageTransition = $state(true);
  let customizationHistory = $state<AppSettings[]>([]);
  let customizationGestureStart: AppSettings | null = null;
  let reordering = $state(false);
  let confirmationMessage = $state<string | null>(null);
  let resetConfirmationOpen = $state(false);
  let resettingCustomization = $state(false);
  let showAbout = $state(false);
  let shareMenuOpen = $state(false);
  let optionsMenuElement = $state<HTMLDetailsElement>();
  let shareTimer: ReturnType<typeof setTimeout> | undefined;
  const providerStates = $derived(Object.values(viewState.providers));
  const anyRefreshing = $derived(providerStates.some((state) => state.refreshing));
  const lastFullRefresh = $derived(viewState.lastFullRefreshAt ?? undefined);
  const platform = desktopPlatform();
  const shortcuts = shortcutLabels(platform);
  const settingsController = new SettingsController((message) => (settingsError = message));
  const providerDisplayName = (id: string) => catalog.displayName(id);
  const settingsState = $derived(settingsController.state);
  const updates = new UpdateController();
  const windowController = createWindowController({
    screen: () => screen,
    refreshing: () => anyRefreshing,
    reordering: () => reordering,
    reducedMotion: () => reducedMotion,
    onError: (message) => (settingsError = message),
  });

  $effect(() => {
    if (!settingsState) return;
    const root = document.documentElement;
    if (settingsState.settings.theme === 'system') delete root.dataset.theme;
    else root.dataset.theme = settingsState.settings.theme;
    root.dataset.density = settingsState.settings.density;
  });

  $effect(() => {
    if (!automaticUpdatesReady || !settingsState?.settings.autoCheckUpdates) return;
    const delay = automaticUpdateDelay(settingsState.settings.lastUpdateCheckAt);
    let interval: ReturnType<typeof setInterval> | undefined;
    const timer = setTimeout(() => {
      void checkForUpdates();
      interval = setInterval(() => void checkForUpdates(), UPDATE_CHECK_INTERVAL_MS);
    }, delay);
    return () => {
      clearTimeout(timer);
      if (interval) clearInterval(interval);
    };
  });

  function scheduleWindowFit() {
    windowController.scheduleFit();
  }

  function beginContentMorph() {
    windowController.beginContentMorph();
  }

  function closePopup() {
    resetTransientUi();
    navigate('dashboard');
    void dismissMainWindow();
  }
  function resetTransientUi() {
    showAbout = false;
    resetConfirmationOpen = false;
    resettingCustomization = false;
    confirmationMessage = null;
    shareMenuOpen = false;
    const content = document.querySelector<HTMLElement>('.content');
    if (content && typeof content.scrollTo === 'function') content.scrollTo({ top: 0 });
    else if (content) content.scrollTop = 0;
  }
  function quitApp() {
    void quitApplication();
  }
  function screenRank(value: Screen) {
    if (value.startsWith('provider:')) return 2;
    return value === 'dashboard' ? 0 : 1;
  }
  function navigate(next: Screen) {
    if (next === screen) return;
    slidePageTransition = shouldSlideBetweenScreens(screen, next);
    slideDirection = screenRank(next) >= screenRank(screen) ? 1 : -1;
    screen = next;
  }
  function back() {
    if (screen.startsWith('provider:')) navigate('customize');
    else if (screen !== 'dashboard') navigate('dashboard');
    else closePopup();
  }
  function saveSettings(next: AppSettings) {
    settingsError = null;
    settingsController.save(next);
  }
  function cloneSettings(value: AppSettings): AppSettings {
    return JSON.parse(JSON.stringify(value)) as AppSettings;
  }
  function showConfirmation(message: string) {
    confirmationMessage = message;
    if (shareTimer) clearTimeout(shareTimer);
    shareTimer = setTimeout(() => (confirmationMessage = null), 1800);
  }
  function saveCustomization(next: AppSettings) {
    const current = settingsState;
    if (!current) return;
    if (customizationGestureStart) {
      settingsController.setState({ ...current, settings: next });
      settingsError = null;
      return;
    }
    customizationHistory = [...customizationHistory.slice(-19), cloneSettings(current.settings)];
    saveSettings(next);
  }
  function beginCustomizationGesture() {
    if (!settingsState) return;
    customizationGestureStart ??= cloneSettings(settingsState.settings);
    reordering = true;
    scheduleWindowFit();
  }
  function endCustomizationGesture(moved: boolean, cancelled = false) {
    const current = settingsState;
    if (!current) return;
    const start = customizationGestureStart;
    const final = current.settings;
    customizationGestureStart = null;
    reordering = false;
    if (start && moved && cancelled) settingsController.setState({ ...current, settings: start });
    else if (start && moved) {
      customizationHistory = [...customizationHistory.slice(-19), start];
      saveSettings(final);
    }
    queueMicrotask(scheduleWindowFit);
  }
  function undoCustomization() {
    const previous = customizationHistory.at(-1);
    if (!previous) return;
    customizationHistory = customizationHistory.slice(0, -1);
    saveSettings(previous);
  }
  async function refresh() {
    if (anyRefreshing) return;
    viewState = {
      providers: Object.fromEntries(
        Object.entries(viewState.providers).map(([id, state]) => [
          id,
          { ...state, refreshing: true, error: null },
        ]),
      ),
    };
    try {
      viewState = await refreshUsage();
    } catch {
      viewState = {
        providers: Object.fromEntries(
          Object.entries(viewState.providers).map(([id, state]) => [
            id,
            { ...state, refreshing: false },
          ]),
        ),
      };
      settingsError = 'OpenQuota could not start a provider refresh.';
    }
  }
  async function refreshProvider(providerId: string) {
    const current = viewState.providers[providerId];
    if (!current || current.refreshing) return;
    viewState = {
      providers: {
        ...viewState.providers,
        [providerId]: { ...current, refreshing: true, error: null },
      },
    };
    try {
      viewState = await refreshProviderUsage(providerId);
    } catch {
      const failed = viewState.providers[providerId];
      if (failed) {
        viewState = {
          providers: {
            ...viewState.providers,
            [providerId]: { ...failed, refreshing: false },
          },
        };
      }
      settingsError = `${providerDisplayName(providerId)} usage could not be refreshed.`;
    }
  }
  function openProviderLink(providerId: string, linkIndex: number) {
    void openProviderLinkCommand(providerId, linkIndex).catch(() => {});
  }
  function requestCustomizationReset() {
    resetConfirmationOpen = true;
  }
  async function confirmCustomizationReset() {
    const current = settingsState;
    if (!current || resettingCustomization) return;
    resettingCustomization = true;
    customizationHistory = [...customizationHistory.slice(-19), cloneSettings(current.settings)];
    try {
      settingsController.setState(await resetCustomizationCommand());
    } catch {
      settingsError = 'Customization could not be reset.';
    } finally {
      resettingCustomization = false;
      resetConfirmationOpen = false;
    }
  }
  async function resetProviderCustomization(providerId: string) {
    const current = settingsState;
    if (!current) return;
    const provider = current.settings.providers.find((item) => item.id === providerId);
    if (!provider) return;
    customizationHistory = [...customizationHistory.slice(-19), cloneSettings(current.settings)];
    try {
      settingsController.setState(await resetProviderCustomizationCommand(providerId));
    } catch {
      settingsError = `${providerDisplayName(providerId)} customization could not be reset.`;
    }
  }
  async function copyCanvas(canvas: HTMLCanvasElement, fallback: string) {
    const blob = await new Promise<Blob>((resolve, reject) =>
      canvas.toBlob(
        (value) => (value ? resolve(value) : reject(new Error('PNG unavailable'))),
        'image/png',
      ),
    );
    if (typeof ClipboardItem !== 'undefined' && navigator.clipboard.write) {
      await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
    } else {
      await navigator.clipboard.writeText(fallback);
    }
    showConfirmation('Copied to clipboard');
  }
  async function shareProvider(providerId: string) {
    const current = settingsState;
    if (!current) return;
    const card = document.querySelector<HTMLElement>(`[data-provider-id="${providerId}"]`);
    if (!card) return;
    const provider = viewState.providers[providerId]?.snapshot;
    const layout = current.settings.providers.find((item) => item.id === providerId);
    if (!provider || !layout) return;
    const snapshot = [providerDisplayName(providerId), card.innerText.trim()].join('\n');
    try {
      const rows = buildProviderShareRows(catalog, provider, layout, current.settings, now);
      const canvas = renderProviderShareCard(catalog, { providerId, plan: provider.plan, rows });
      await copyCanvas(canvas, snapshot);
    } catch {
      settingsError = 'Provider screenshot could not be copied.';
    }
  }
  async function shareTotalSpend(projection: SpendProjection) {
    const current = settingsState;
    if (!current) return false;
    const card = document.querySelector<HTMLElement>('[data-total-spend]');
    if (!card) return false;
    try {
      const canvas = renderTotalSpendShareCard(catalog, {
        projection,
        metric: current.settings.totalSpendMetric,
        period: current.settings.totalSpendPeriod,
      });
      await copyCanvas(canvas, card.innerText.trim());
      return true;
    } catch {
      settingsError = 'Total Spend screenshot could not be copied.';
      return false;
    }
  }
  async function copyLogPath() {
    const path = await getLogPath();
    await navigator.clipboard.writeText(path);
    showConfirmation('Log path copied');
  }
  async function openLogFolder() {
    await openSystemLogFolder();
  }
  function topBarTitle() {
    if (screen.startsWith('provider:')) return providerDisplayName(screen.slice(9));
    return screen === 'settings' ? 'Settings' : 'Customize';
  }
  function closeAboutFromBackdrop(event: MouseEvent) {
    if (event.target === event.currentTarget) showAbout = false;
  }
  function ownsEnterKey(target: EventTarget | null) {
    if (!(target instanceof Element)) return false;
    return (
      target.closest(
        'button, a, input, select, textarea, summary, [contenteditable], [role="button"], [role="menuitem"], [role="option"], [role="combobox"]',
      ) !== null
    );
  }
  function handleOptionsKey(event: KeyboardEvent) {
    const menu = (event.currentTarget as HTMLElement).closest<HTMLDetailsElement>(
      'details.options-menu',
    );
    if (!menu) return;
    if (event.key !== 'Escape' || !menu.open) return;
    event.preventDefault();
    event.stopPropagation();
    menu.open = false;
    menu.querySelector<HTMLElement>('summary')?.focus();
  }
  function closeOptionsMenu(restoreFocus = false) {
    if (!optionsMenuElement?.open) return;
    optionsMenuElement.open = false;
    if (restoreFocus) optionsMenuElement.querySelector<HTMLElement>('summary')?.focus();
  }
  function handleWindowPointerDown(event: PointerEvent) {
    if (
      optionsMenuElement?.open &&
      event.target instanceof Node &&
      !optionsMenuElement.contains(event.target)
    ) {
      closeOptionsMenu();
    }
  }
  async function requestNotifications() {
    const current = settingsState;
    if (!current) return;
    try {
      const currentSettings = current.settings;
      const permissionState = await requestNotificationPermission();
      settingsController.setState({ ...permissionState, settings: currentSettings });
    } catch {
      settingsError = 'Notification permission could not be requested.';
    }
  }
  async function openNotificationSettings() {
    try {
      await openSystemNotificationSettings();
    } catch {
      settingsError = 'Notification settings could not be opened on this system.';
    }
  }
  async function checkForUpdates(manual = false) {
    if (!settingsState) return;
    await updates.check(
      manual,
      (checkedAt) => {
        if (!settingsState) return;
        saveSettings({ ...settingsState.settings, lastUpdateCheckAt: checkedAt });
      },
      showConfirmation,
    );
  }

  onMount(() => {
    const motionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
    const updateMotionPreference = () => {
      reducedMotion = motionQuery.matches;
      document.documentElement.toggleAttribute('data-reduced-motion', reducedMotion);
      scheduleWindowFit();
    };
    updateMotionPreference();
    motionQuery.addEventListener('change', updateMotionPreference);
    const refreshPermissionState = () => {
      void settingsController.refreshIfIdle();
    };
    window.addEventListener('focus', refreshPermissionState);

    const popover = document.querySelector<HTMLElement>('.popover');
    const resizeObserver =
      typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(scheduleWindowFit);
    const observePanelParts = () => {
      resizeObserver?.disconnect();
      document
        .querySelectorAll<HTMLElement>('.screen-page, .screen-header, .footer, .notice')
        .forEach((element) => resizeObserver?.observe(element));
      scheduleWindowFit();
    };
    const mutationObserver = new MutationObserver(observePanelParts);
    if (popover) {
      mutationObserver.observe(popover, { childList: true, subtree: true, characterData: true });
    }
    observePanelParts();
    const handleKeydown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        if (showAbout) {
          showAbout = false;
          return;
        }
        back();
      } else if (event.key === 'Enter' && screen === 'dashboard' && !ownsEnterKey(event.target)) {
        event.preventDefault();
        navigate('customize');
      } else if ((event.ctrlKey || event.metaKey) && event.key === ',') {
        event.preventDefault();
        navigate(screen === 'settings' ? 'dashboard' : 'settings');
      } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'r') {
        event.preventDefault();
        void refresh();
      } else if (
        (event.ctrlKey || event.metaKey) &&
        event.key.toLowerCase() === 'z' &&
        !(event.target instanceof HTMLInputElement) &&
        !(event.target instanceof HTMLTextAreaElement)
      ) {
        event.preventDefault();
        undoCustomization();
      } else if (
        (event.ctrlKey || event.metaKey) &&
        event.key.toLowerCase() === 'q' &&
        !(event.target instanceof HTMLInputElement) &&
        !(event.target instanceof HTMLTextAreaElement)
      ) {
        event.preventDefault();
        quitApp();
      }
    };
    document.addEventListener('keydown', handleKeydown);
    const clock = window.setInterval(() => (now = Date.now()), 30_000);
    const listeners = createListenerRegistry(() => {
      settingsError ??= 'OpenQuota event bridge is unavailable.';
    });
    listeners.add(onUsageState((state) => (viewState = state)));
    listeners.add(
      onSettingsState((state) => {
        settingsController.acceptExternalState(state);
      }),
    );
    listeners.add(
      onOpenScreen((target) => navigate(target === 'settings' ? 'settings' : 'customize')),
    );
    listeners.add(
      onPopupHidden(() => {
        resetTransientUi();
        navigate('dashboard');
      }),
    );
    listeners.add(
      onUpdateProgress((progress) => {
        updates.setProgress(progress);
      }),
    );
    void getBootstrapState()
      .then((state) => {
        catalog = new ProviderCatalogIndex(state.catalog);
        viewState = state.usage;
        settingsController.setState(state.settings);
        automaticUpdatesReady = true;
      })
      .catch(() => (settingsError = 'OpenQuota backend is unavailable.'));
    return () => {
      document.removeEventListener('keydown', handleKeydown);
      window.clearInterval(clock);
      windowController.dispose();
      motionQuery.removeEventListener('change', updateMotionPreference);
      window.removeEventListener('focus', refreshPermissionState);
      document.documentElement.removeAttribute('data-reduced-motion');
      mutationObserver.disconnect();
      resizeObserver?.disconnect();
      listeners.dispose();
    };
  });
</script>

<svelte:head><meta name="color-scheme" content="light dark" /></svelte:head>
<svelte:window onpointerdown={handleWindowPointerDown} />

<main
  class="popover"
  aria-label="OpenQuota usage dashboard"
  oncontextmenu={(event) => event.preventDefault()}
>
  <p id="reorder-instructions" class="sr-only">
    Drag to reorder. With a keyboard, use Alt plus Up Arrow or Alt plus Down Arrow.
  </p>
  {#if settingsState}
    {#if screen !== 'dashboard'}
      <header class="screen-header app-top-bar">
        <button type="button" onclick={back} aria-label="Back" data-tooltip="Back">
          <Icon name="back" size={16} strokeWidth={2.2} />
        </button>
        <h1>{topBarTitle()}</h1>
        {#if screen === 'customize'}
          <button
            class="text-button"
            type="button"
            onclick={requestCustomizationReset}
            aria-label="Reset all customization"
            data-tooltip="Reset All Customization"
            ><Icon name="reset" size={15} strokeWidth={2} /></button
          >
        {:else if screen.startsWith('provider:')}
          <button
            class="text-button"
            type="button"
            onclick={() => resetProviderCustomization(screen.slice(9))}
            aria-label={`Reset ${topBarTitle()}`}
            data-tooltip={`Reset ${topBarTitle()}`}
            ><Icon name="reset" size={15} strokeWidth={2} /></button
          >
        {:else}
          <span></span>
        {/if}
      </header>
    {/if}
    <div class="content" class:content--chrome={screen !== 'dashboard'}>
      {#if settingsError}<div class="notice notice--blocking" role="alert">
          {settingsError}
        </div>{/if}
      <div class="screen-stage">
        {#key screen}
          <div
            class="screen-page"
            data-screen={screen}
            in:horizontalPageTransition={{
              direction: slideDirection,
              ...springMotion(reducedMotion || !slidePageTransition),
            }}
            out:horizontalPageTransition={{
              direction: -slideDirection,
              ...springMotion(reducedMotion || !slidePageTransition),
            }}
          >
            {#if screen === 'dashboard'}
              <Dashboard
                {viewState}
                {catalog}
                settings={settingsState.settings}
                {now}
                onSettingsChange={saveSettings}
                onCustomizationChange={saveCustomization}
                onReorderStart={beginCustomizationGesture}
                onReorderEnd={endCustomizationGesture}
                onCustomize={() => navigate('customize')}
                onOpenProviderCustomize={(id) => navigate(`provider:${id}`)}
                onShare={shareProvider}
                onShareTotal={shareTotalSpend}
                onRefresh={refreshProvider}
                onOpenProviderLink={openProviderLink}
                onContentMorph={beginContentMorph}
                {reducedMotion}
                updateStatus={updates.status}
                installingUpdate={updates.installing}
                updateProgress={updates.progress}
                updateError={updates.error}
                onInstallUpdate={() => updates.install()}
                onOpenUpdatePage={() => updates.openDownloadPage()}
              />
            {:else if screen === 'settings'}
              <SettingsScreen
                settingsView={settingsState}
                {platform}
                onChange={saveSettings}
                onRequestNotifications={requestNotifications}
                onOpenNotificationSettings={openNotificationSettings}
                updateError={updates.error}
                checkingUpdate={updates.checking}
                onCheckForUpdates={() => void checkForUpdates(true)}
                onCustomize={() => navigate('customize')}
                onCopyLogPath={copyLogPath}
                onOpenLogFolder={openLogFolder}
              />
            {:else if screen === 'customize'}
              <CustomizeProviderList
                settings={settingsState.settings}
                {catalog}
                onOpen={(id) => navigate(`provider:${id}`)}
                onChange={saveCustomization}
                onReorderStart={beginCustomizationGesture}
                onReorderEnd={endCustomizationGesture}
                onSettings={() => navigate('settings')}
                {reducedMotion}
              />
            {:else if screen.startsWith('provider:')}
              <CustomizeProviderDetail
                settings={settingsState.settings}
                providerId={screen.slice(9)}
                {catalog}
                onChange={saveCustomization}
                onReorderStart={beginCustomizationGesture}
                onReorderEnd={endCustomizationGesture}
                {reducedMotion}
              />
            {/if}
          </div>
        {/key}
      </div>
    </div>

    {#if screen === 'dashboard' || screen === 'settings'}
      <footer class="footer">
        <button
          class="identity"
          type="button"
          onclick={refresh}
          disabled={anyRefreshing}
          aria-label="Refresh all provider usage"
        >
          <span>OpenQuota {appVersion}</span><small
            >{anyRefreshing ? 'Updating…' : nextUpdateLabel(lastFullRefresh, now)}</small
          >
        </button>
        {#if screen === 'dashboard'}
          <details class="options-menu" bind:this={optionsMenuElement}>
            <summary aria-label="Open options" onkeydown={handleOptionsKey}
              ><span>Options</span><Icon name="chevron-down" size={11} strokeWidth={2.2} /></summary
            >
            <div
              class="options-menu__panel"
              role="menu"
              aria-label="Options menu"
              tabindex="-1"
              onkeydown={handleOptionsKey}
              onclick={(event) => {
                if (event.target instanceof Element && event.target.closest('button')) {
                  closeOptionsMenu();
                }
              }}
            >
              <button
                class="menu-item"
                type="button"
                aria-label="Customize"
                onclick={() => navigate('customize')}
                ><Icon name="sliders" /><span>Customize</span><kbd>↩</kbd></button
              >
              <button
                class="menu-item"
                type="button"
                aria-label="Settings"
                onclick={() => navigate('settings')}
                ><Icon name="gear" /><span>Settings</span><kbd>{shortcuts.settings}</kbd></button
              >
              <hr />
              <details
                class="share-menu"
                ontoggle={(event) => (shareMenuOpen = event.currentTarget.open)}
              >
                <summary
                  ><span class="share-menu__direction"><Icon name="chevron-left" size={12} /></span
                  ><span>Share Screenshot</span></summary
                >
                <div>
                  {#if shareMenuOpen}
                    {#each settingsState.settings.providers.filter((provider) => provider.enabled) as provider (provider.id)}
                      <button type="button" onclick={() => shareProvider(provider.id)}
                        >{providerDisplayName(provider.id)}</button
                      >
                    {/each}
                  {/if}
                </div>
              </details>
              <button class="menu-item" type="button" onclick={() => void checkForUpdates(true)}
                ><Icon name="refresh" /><span>Check for Updates…</span></button
              >
              <hr />
              <button class="menu-item" type="button" onclick={() => (showAbout = true)}
                ><Icon name="about" /><span>About OpenQuota</span></button
              >
              <button
                class="menu-item menu-item--danger"
                type="button"
                aria-label="Quit OpenQuota"
                onclick={quitApp}
                ><Icon name="power" /><span>Quit OpenQuota</span><kbd>{shortcuts.quit}</kbd></button
              >
            </div>
          </details>
        {/if}
      </footer>
    {/if}

    {#if confirmationMessage}
      <div class="transient-pill" role="status">
        <Icon name="check" size={15} strokeWidth={2.4} />{confirmationMessage}
      </div>
    {/if}

    {#if resetConfirmationOpen}
      <ConfirmationSheet
        title="Reset All Customization?"
        message="This turns installed providers back on and restores every provider's metric visibility and order."
        confirmLabel="Reset All"
        pending={resettingCustomization}
        onConfirm={() => void confirmCustomizationReset()}
        onCancel={() => (resetConfirmationOpen = false)}
      />
    {/if}

    {#if showAbout}
      <div class="about-backdrop" role="presentation" onclick={closeAboutFromBackdrop}>
        <div
          class="about-card"
          role="dialog"
          tabindex="-1"
          aria-modal="true"
          aria-label="About OpenQuota"
        >
          <button
            class="about-card__close"
            type="button"
            aria-label="Close About"
            onclick={() => (showAbout = false)}
            ><Icon name="close" size={11} strokeWidth={2.3} /></button
          >
          <OpenQuotaMark size={44} />
          <h1>OpenQuota</h1>
          <p>Version {appVersion}</p>
          <small>Private, local usage monitoring for your AI coding tools.</small>
        </div>
      </div>
    {/if}
  {:else}
    <div class="content">
      {#if settingsError}
        <div class="notice notice--blocking" role="alert">{settingsError}</div>
      {:else}
        <p class="empty-row">Loading OpenQuota…</p>
      {/if}
    </div>
  {/if}
</main>

<style>
  :global {
    .popover {
      position: relative;
      display: flex;
      width: 100%;
      height: 100%;
      flex-direction: column;
      overflow: hidden;
      color: var(--text);
      background: var(--tray);
      isolation: isolate;
      user-select: none;
    }

    .content {
      flex: 1;
      min-height: 0;
      padding: 14px 14px 12px;
      overflow-y: auto;
      scrollbar-width: none;
      overflow-x: hidden;
    }

    .content::-webkit-scrollbar {
      width: 0;
      height: 0;
    }

    .footer {
      display: flex;
      min-height: 58px;
      align-items: center;
      gap: 12px;
      padding: 10px 14px;
      border-top: 1px solid var(--separator);
      background: color-mix(in srgb, var(--tray) 92%, transparent);
    }

    .identity {
      display: flex;
      flex-direction: column;
      color: var(--secondary);
      font-size: 10px;
      line-height: 14px;
    }

    .identity small {
      color: var(--tertiary);
      font: inherit;
    }

    .options-menu {
      position: relative;
    }

    .options-menu > summary {
      display: grid;
      width: 30px;
      height: 30px;
      border-radius: 50%;
      color: var(--secondary);
      cursor: pointer;
      font-size: 13px;
      list-style: none;
      place-items: center;
    }

    .options-menu > summary::-webkit-details-marker {
      display: none;
    }

    .options-menu[open] > summary,
    .options-menu > summary:hover {
      color: var(--text);
      background: var(--button-hover);
    }

    .options-menu > div {
      position: absolute;
      right: 0;
      bottom: 36px;
      z-index: 10;
      width: 130px;
      padding: 4px;
      border: 1px solid var(--separator);
      border-radius: 9px;
      background: var(--tray);
      box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
    }

    .options-menu button {
      width: 100%;
      padding: 6px 8px;
      border: 0;
      border-radius: 6px;
      color: var(--text);
      background: none;
      font-size: 11px;
      text-align: left;
    }

    .options-menu button:hover {
      background: var(--button-hover);
    }

    .screen-header {
      display: grid;
      min-height: 30px;
      align-items: center;
      grid-template-columns: 54px 1fr 54px;
      margin-bottom: 8px;
    }

    .screen-header h1 {
      margin: 0;
      font-size: 14px;
      text-align: center;
    }

    .screen-header button {
      width: fit-content;
      padding: 3px 7px;
      border: 0;
      border-radius: 6px;
      color: var(--secondary);
      background: transparent;
      cursor: pointer;
    }

    .screen-header > button:first-child {
      font-size: 23px;
      line-height: 20px;
    }

    .screen-header .text-button {
      justify-self: end;
      color: var(--meter-fill);
      font-size: 10px;
    }

    .screen-header button:hover {
      background: var(--button-hover);
    }

    :root[data-density='compact'] .content {
      padding: 9px 11px 7px;
    }

    :root[data-density='compact'] .footer {
      min-height: 48px;
      padding-top: 6px;
      padding-bottom: 6px;
    }

    .content {
      padding: 14px 14px 12px;
      scrollbar-width: none;
    }

    .content--chrome {
      padding-top: 12px;
    }

    .screen-stage {
      display: grid;
      width: 100%;
      min-width: 0;
      min-height: 0;
      overflow: clip;
      background: var(--tray);
    }

    .screen-page {
      width: 100%;
      min-width: 0;
      min-height: 0;
      grid-area: 1 / 1;
      align-self: start;
      transform-origin: 50% 45%;
    }

    .footer {
      min-height: 52px;
      padding: 12px 14px;
      border-top: 0;
      background: color-mix(in srgb, var(--tray) 94%, transparent);
      box-shadow: 0 -10px 18px -18px rgba(0, 0, 0, 0.65);
    }

    .identity {
      padding: 0;
      border: 0;
      color: var(--secondary);
      background: none;
      font-size: 10px;
      line-height: 12px;
      text-align: left;
      cursor: pointer;
    }

    .identity:disabled {
      cursor: default;
    }

    .options-menu {
      margin-left: auto;
    }

    .options-menu > summary {
      display: flex;
      width: auto;
      height: 26px;
      align-items: center;
      gap: 4px;
      padding: 0 9px 0 10px;
      border: 1px solid var(--separator);
      border-radius: 8px;
      color: var(--text);
      background: color-mix(in srgb, var(--card) 72%, transparent);
      font-size: 11px;
      font-weight: 550;
    }

    .options-menu > summary i {
      margin-top: -2px;
      font-size: 11px;
      font-style: normal;
    }

    .options-menu > summary .symbol-icon {
      transition: transform 160ms ease;
    }

    .options-menu[open] > summary .symbol-icon {
      transform: rotate(180deg);
    }

    .options-menu > div {
      bottom: 34px;
      width: 172px;
      padding: 6px;
      border: 0;
      border-radius: 10px;
      box-shadow: 0 10px 32px rgba(0, 0, 0, 0.28);
      transform-origin: bottom right;
      animation: menu-in 180ms ease-out both;
    }

    .options-menu button {
      font-size: 11px;
    }

    .screen-header {
      position: sticky;
      top: 0;
      z-index: 5;
      min-height: 44px;
      grid-template-columns: 44px 1fr 44px;
      margin: 0 -14px 12px;
      padding: 0 14px;
      background: color-mix(in srgb, var(--tray) 94%, transparent);
      box-shadow: 0 10px 18px -20px rgba(0, 0, 0, 0.8);
      backdrop-filter: blur(18px);
    }

    .app-top-bar {
      position: relative;
      top: auto;
      z-index: 10;
      width: 100%;
      min-height: 44px;
      flex: 0 0 44px;
      margin: 0;
      padding: 0 14px;
    }

    .screen-header h1 {
      font-size: 13px;
      font-weight: 600;
    }

    .screen-header button:first-child {
      display: grid;
      width: 28px;
      height: 28px;
      padding: 0;
      border-radius: 50%;
      background: var(--button-hover);
      place-items: center;
    }

    .screen-header .text-button {
      width: 28px;
      height: 28px;
      overflow: hidden;
      color: var(--secondary);
      font-size: inherit;
    }

    .screen-header .text-button::after {
      content: none;
    }

    .options-menu .menu-item,
    .share-menu > summary {
      display: flex;
      width: 100%;
      min-height: 32px;
      align-items: center;
      gap: 8px;
      padding: 7px 9px;
      border: 0;
      border-radius: 6px;
      color: var(--text);
      background: transparent;
      font-size: 11px;
      text-align: left;
    }

    .options-menu .menu-item span,
    .share-menu > summary span {
      flex: 1;
    }

    .options-menu kbd {
      color: var(--tertiary);
      background: none;
      font: 10px/1 inherit;
    }

    .options-menu .menu-item--danger {
      color: var(--meter-critical);
    }

    .share-menu {
      position: relative;
    }

    .share-menu > summary {
      cursor: pointer;
      list-style: none;
    }

    .share-menu > summary::-webkit-details-marker {
      display: none;
    }

    .share-menu > summary .share-menu__direction {
      display: grid;
      width: 16px;
      flex: 0 0 16px;
      place-items: center;
    }

    .share-menu > summary .share-menu__direction .symbol-icon {
      transition: transform 140ms ease;
    }

    .share-menu[open] > summary .share-menu__direction .symbol-icon {
      transform: translateX(-2px);
    }

    .share-menu > div {
      position: absolute;
      right: calc(100% - 2px);
      bottom: -5px;
      width: 130px;
      max-width: calc(100vw - 16px);
      padding: 5px;
      border: 1px solid var(--separator);
      border-radius: 9px;
      background: var(--tray);
      box-shadow: 0 8px 24px rgba(0, 0, 0, 0.24);
      transform-origin: bottom right;
      animation: menu-in 160ms ease-out both;
    }

    .share-menu button {
      width: 100%;
      min-height: 30px;
      padding: 7px 9px;
      border: 0;
      border-radius: 5px;
      color: var(--text);
      background: transparent;
      font-size: 11px;
      text-align: left;
    }

    .transient-pill {
      position: absolute;
      right: 14px;
      bottom: 62px;
      z-index: 90;
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 7px 10px;
      border: 1px solid var(--separator);
      border-radius: 999px;
      color: var(--text);
      background: color-mix(in srgb, var(--tray) 96%, transparent);
      box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
      font-size: 10px;
      animation: detail-in var(--motion-spring) both;
    }

    .transient-pill .symbol-icon {
      color: #34c759;
    }

    .about-backdrop {
      position: absolute;
      z-index: 100;
      display: grid;
      border: 0;
      background: rgba(0, 0, 0, 0.28);
      inset: 0;
      place-items: center;
      backdrop-filter: blur(6px);
    }

    .about-card {
      position: relative;
      display: flex;
      width: 230px;
      align-items: center;
      padding: 24px 20px 20px;
      border: 1px solid var(--separator);
      border-radius: 16px;
      color: var(--text);
      background: var(--tray);
      box-shadow: 0 18px 55px rgba(0, 0, 0, 0.35);
      flex-direction: column;
      animation: detail-in var(--motion-spring) both;
    }

    .about-card h1 {
      margin: 10px 0 2px;
      font-size: 17px;
    }

    .about-card p,
    .about-card small {
      margin: 0;
      color: var(--secondary);
      font-size: 10px;
      text-align: center;
    }

    .about-card__close {
      position: absolute;
      top: 8px;
      right: 8px;
      display: grid;
      width: 24px;
      height: 24px;
      padding: 0;
      border: 0;
      border-radius: 50%;
      color: var(--secondary);
      background: var(--button-hover);
      cursor: pointer;
      place-items: center;
      transition:
        color var(--motion-switch),
        background var(--motion-switch),
        transform var(--motion-switch);
    }

    .about-card__close:hover {
      color: var(--text);
      background: color-mix(in srgb, var(--text) 14%, transparent);
    }

    .about-card__close:active {
      transform: scale(0.92);
    }

    .about-card__close:focus-visible {
      outline: 2px solid color-mix(in srgb, var(--meter-fill) 55%, transparent);
      outline-offset: 1px;
    }

    :root[data-density='compact'] .content {
      padding: 10px 14px 8px;
    }

    :root[data-density='compact'] .content--chrome {
      padding-top: 12px;
    }

    .notice--blocking {
      color: var(--error);
      background: var(--error-bg);
    }

    .popover {
      width: 320px;
      max-width: 320px;
    }
  }
</style>
