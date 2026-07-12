<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { currentMonitor, getCurrentWindow } from '@tauri-apps/api/window';
  import CustomizeProviderDetail from './lib/CustomizeProviderDetail.svelte';
  import CustomizeProviderList from './lib/CustomizeProviderList.svelte';
  import Dashboard from './lib/Dashboard.svelte';
  import Icon from './lib/Icon.svelte';
  import { defaultMetricLayout, metricDefinition, providerDisplayName } from './lib/metrics';
  import OpenQuotaMark from './lib/OpenQuotaMark.svelte';
  import { horizontalPageTransition, shouldSlideBetweenScreens } from './lib/pageTransition';
  import { panelTargetHeight, screenPanelHeight } from './lib/panelSizing';
  import { desktopPlatform, shortcutLabels } from './lib/platform';
  import { providerIconPath } from './lib/providerIconPaths';
  import SettingsScreen from './lib/SettingsScreen.svelte';
  import type { SpendProjection } from './lib/totalSpend';
  import type {
    AppSettings,
    QuotaWindow,
    SettingsViewState,
    UpdateProgress,
    UpdateStatus,
    UsageViewState,
  } from './lib/types';
  import { automaticUpdateDelay, UPDATE_CHECK_INTERVAL_MS } from './lib/updateSchedule';

  type Screen = 'dashboard' | 'customize' | 'settings' | `provider:${string}`;
  type ShareRow =
    | { kind: 'quota'; label: string; quota: QuotaWindow }
    | { kind: 'text'; label: string; value: string };
  const emptyView: UsageViewState = { providers: {} };
  const emptySettings: SettingsViewState = {
    notificationPermission: 'prompt',
    integrationError: null,
    standaloneWindow: false,
    platformSummary: null,
    settings: {
      schemaVersion: 4,
      knownProviderIds: ['claude', 'codex', 'antigravity'],
      showTotalSpend: true,
      theme: 'system',
      density: 'default',
      menuBarStyle: 'text',
      usageDisplay: 'left',
      resetDisplay: 'countdown',
      timeFormat: 'system',
      alwaysShowPacing: false,
      launchAtLogin: false,
      autoCheckUpdates: true,
      dismissedUpdateVersion: null,
      lastUpdateCheckAt: null,
      globalShortcut: null,
      notifications: { almostOut: false, cuttingItClose: false, willRunOut: false },
      totalSpendMetric: 'cost',
      totalSpendPeriod: 'today',
      detectionNoticeDismissed: false,
      providers: [{ id: 'codex', enabled: true, detected: false, expanded: false, metrics: [] }],
    },
  };

  let viewState = $state<UsageViewState>(emptyView);
  let settingsState = $state<SettingsViewState>(emptySettings);
  let screen = $state<Screen>('dashboard');
  let now = $state(Date.now());
  let settingsError = $state<string | null>(null);
  let updateStatus = $state<UpdateStatus | null>(null);
  let updateError = $state<string | null>(null);
  let checkingUpdate = $state(false);
  let installingUpdate = $state(false);
  let updateProgress = $state<UpdateProgress | null>(null);
  let automaticUpdatesReady = $state(false);
  let saveQueue: Promise<void> = Promise.resolve();
  let measureFrame = 0;
  let resizeFrame = 0;
  let resizeGeneration = 0;
  let windowResizeAvailable = true;
  let dashboardPanelHeight: number | null = null;
  let reducedMotion = $state(false);
  let slideDirection = $state(1);
  let slidePageTransition = $state(true);
  let customizationHistory = $state<AppSettings[]>([]);
  let confirmationMessage = $state<string | null>(null);
  let showAbout = $state(false);
  let shareMenuOpen = $state(false);
  let optionsMenuElement = $state<HTMLDetailsElement>();
  let shareTimer: ReturnType<typeof setTimeout> | undefined;
  const providerStates = $derived(Object.values(viewState.providers));
  const anyRefreshing = $derived(providerStates.some((state) => state.refreshing));
  const latestRefresh = $derived.by(() =>
    providerStates
      .map((state) => state.snapshot?.refreshedAt)
      .filter((value): value is string => value !== undefined)
      .sort()
      .at(-1),
  );
  const platform = desktopPlatform();
  const shortcuts = shortcutLabels(platform);

  $effect(() => {
    const root = document.documentElement;
    if (settingsState.settings.theme === 'system') delete root.dataset.theme;
    else root.dataset.theme = settingsState.settings.theme;
    root.dataset.density = settingsState.settings.density;
  });

  $effect(() => {
    if (!automaticUpdatesReady || !settingsState.settings.autoCheckUpdates) return;
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
    if (
      typeof window === 'undefined' ||
      !('__TAURI_INTERNALS__' in window) ||
      !windowResizeAvailable
    )
      return;
    window.cancelAnimationFrame(measureFrame);
    measureFrame = window.requestAnimationFrame(() => void fitWindowToScreen());
  }

  async function fitWindowToScreen() {
    const page = document.querySelector<HTMLElement>(`.screen-page[data-screen="${screen}"]`);
    const content = document.querySelector<HTMLElement>('.content');
    const stage = document.querySelector<HTMLElement>('.screen-stage');
    const header = document.querySelector<HTMLElement>('.screen-header');
    const footer = document.querySelector<HTMLElement>('.footer');
    if (!page || !content || !stage) return;
    const pageHeight = page.scrollHeight;
    stage.style.height = `${pageHeight}px`;
    const contentStyle = window.getComputedStyle(content);
    const contentPadding =
      Number.parseFloat(contentStyle.paddingTop) + Number.parseFloat(contentStyle.paddingBottom);
    const idealHeight =
      pageHeight + contentPadding + (header?.offsetHeight ?? 0) + (footer?.offsetHeight ?? 0);
    const appWindow = getCurrentWindow();
    const monitor = await currentMonitor().catch(() => null);
    const workAreaHeight = monitor
      ? monitor.workArea.size.height / monitor.scaleFactor
      : window.screen.availHeight;
    const scale = await appWindow.scaleFactor();
    const current = (await appWindow.innerSize()).height / scale;
    const contentTarget = panelTargetHeight(idealHeight, workAreaHeight);
    const target = screenPanelHeight(
      screen,
      contentTarget,
      dashboardPanelHeight ?? Math.round(current),
    );
    if (screen === 'dashboard') dashboardPanelHeight = target;
    const generation = ++resizeGeneration;
    window.cancelAnimationFrame(resizeFrame);
    if (reducedMotion) {
      await resizeWindow(target);
      return;
    }
    if (Math.abs(current - target) < 1) return;
    const started = performance.now();
    const duration = 420;
    const animate = (time: number) => {
      if (generation !== resizeGeneration) return;
      const progress = Math.min(1, (time - started) / duration);
      const eased = 1 - Math.pow(1 - progress, 3);
      void resizeWindow(Math.round(current + (target - current) * eased));
      if (progress < 1) {
        resizeFrame = window.requestAnimationFrame(animate);
      }
    };
    resizeFrame = window.requestAnimationFrame(animate);
  }

  async function resizeWindow(height: number) {
    try {
      await invoke('resize_main_window', { height });
      return true;
    } catch {
      windowResizeAvailable = false;
      settingsError = 'OpenQuota window could not adapt to its content.';
      return false;
    }
  }

  function closePopup() {
    resetTransientUi();
    navigate('dashboard');
    void invoke('dismiss_main_window');
  }
  function resetTransientUi() {
    showAbout = false;
    confirmationMessage = null;
    shareMenuOpen = false;
    const content = document.querySelector<HTMLElement>('.content');
    if (content && typeof content.scrollTo === 'function') content.scrollTo({ top: 0 });
    else if (content) content.scrollTop = 0;
  }
  function quitApp() {
    void invoke('quit_app');
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
  function springOut(progress: number) {
    return 1 - Math.pow(1 - progress, 3);
  }
  function back() {
    if (screen.startsWith('provider:')) navigate('customize');
    else if (screen !== 'dashboard') navigate('dashboard');
    else closePopup();
  }
  function saveSettings(next: AppSettings) {
    settingsState = { ...settingsState, settings: next };
    settingsError = null;
    saveQueue = saveQueue
      .then(async () => {
        settingsState = await invoke<SettingsViewState>('save_app_settings', { settings: next });
      })
      .catch((error: unknown) => {
        settingsError = typeof error === 'string' ? error : 'Settings could not be saved.';
      });
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
    customizationHistory = [
      ...customizationHistory.slice(-19),
      cloneSettings(settingsState.settings),
    ];
    saveSettings(next);
  }
  function undoCustomization() {
    const previous = customizationHistory.at(-1);
    if (!previous) return;
    customizationHistory = customizationHistory.slice(0, -1);
    saveSettings(previous);
  }
  async function refresh() {
    viewState = {
      providers: Object.fromEntries(
        Object.entries(viewState.providers).map(([id, state]) => [
          id,
          { ...state, refreshing: true, error: null },
        ]),
      ),
    };
    try {
      viewState = await invoke<UsageViewState>('refresh_usage');
    } catch {
      settingsError = 'OpenQuota could not start a provider refresh.';
    }
  }
  async function resetCustomization() {
    if (
      !window.confirm(
        "Turns providers back on for the tools you have installed and resets every provider's metrics and order. Are you sure?",
      )
    )
      return;
    customizationHistory = [
      ...customizationHistory.slice(-19),
      cloneSettings(settingsState.settings),
    ];
    try {
      settingsState = await invoke<SettingsViewState>('reset_customization');
    } catch {
      settingsError = 'Customization could not be reset.';
    }
  }
  function resetProviderCustomization(providerId: string) {
    const provider = settingsState.settings.providers.find((item) => item.id === providerId);
    if (!provider) return;
    saveCustomization({
      ...settingsState.settings,
      providers: settingsState.settings.providers.map((item) =>
        item.id === providerId
          ? { ...item, expanded: false, metrics: defaultMetricLayout(providerId) }
          : item,
      ),
    });
  }
  function canvasPalette() {
    const styles = getComputedStyle(document.documentElement);
    const value = (name: string) => styles.getPropertyValue(name).trim();
    return {
      tray: value('--tray'),
      surface: value('--card'),
      text: value('--text'),
      secondary: value('--secondary'),
      track: value('--meter-track'),
      fill: value('--meter-fill'),
      separator: value('--separator'),
      provider: (id: string) => value(`--provider-${id}`) || value('--provider'),
    };
  }
  function drawProviderMark(
    context: CanvasRenderingContext2D,
    providerId: string,
    x: number,
    y: number,
    size: number,
    color: string,
  ) {
    const path = providerIconPath(providerId);
    if (!path || typeof Path2D === 'undefined') return;
    context.save();
    context.translate(x, y);
    context.scale(size / 100, size / 100);
    context.fillStyle = color;
    context.fill(new Path2D(path));
    context.restore();
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
    const card = document.querySelector<HTMLElement>(`[data-provider-id="${providerId}"]`);
    if (!card) return;
    const provider = viewState.providers[providerId]?.snapshot;
    const snapshot = [providerDisplayName(providerId), card.innerText.trim()].join('\n');
    try {
      const layout = settingsState.settings.providers.find((item) => item.id === providerId);
      const visible =
        layout?.metrics.filter(
          (metric) =>
            metric.enabled && (metric.section === 'alwaysVisible' || Boolean(layout.expanded)),
        ) ?? [];
      const rows: ShareRow[] = [];
      for (const metric of visible) {
        const definition = metricDefinition(metric.id);
        if (!definition || !provider) continue;
        if (definition.kind === 'quota') {
          const quota = provider.quotas.find((item) => item.id === definition.sourceId);
          if (quota) rows.push({ kind: 'quota', label: definition.label, quota });
          continue;
        }
        if (definition.kind === 'usage') {
          const period =
            definition.sourceId === 'today'
              ? provider.usage.today
              : definition.sourceId === 'yesterday'
                ? provider.usage.yesterday
                : provider.usage.last30Days;
          const value = period
            ? `${period.estimatedCostUsd === null ? '' : `$${period.estimatedCostUsd.toFixed(2)} · `}${new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(period.tokens)} tokens`
            : 'No data';
          rows.push({ kind: 'text', label: definition.label, value });
          continue;
        }
        const total = provider.usage.daily.reduce((sum, day) => sum + day.tokens, 0);
        rows.push({
          kind: 'text',
          label: definition.label,
          value:
            total > 0
              ? `${new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(total)} tokens`
              : 'No data',
        });
      }
      const rowHeight = (row: ShareRow) => (row.kind === 'quota' ? 92 : 54);
      const contentHeight = rows.reduce((sum, row) => sum + rowHeight(row), 0);
      const canvas = document.createElement('canvas');
      canvas.width = 720;
      canvas.height = Math.max(350, 188 + contentHeight);
      const context = canvas.getContext('2d');
      if (!context) throw new Error('Canvas unavailable');
      const palette = canvasPalette();
      context.fillStyle = palette.tray;
      context.fillRect(0, 0, canvas.width, canvas.height);
      context.fillStyle = palette.text;
      context.font = '600 30px system-ui';
      context.fillText(providerDisplayName(providerId), 42, 58);
      context.fillStyle = palette.secondary;
      context.font = '17px system-ui';
      context.fillText(provider?.plan ?? 'OpenQuota', 42, 86);
      drawProviderMark(
        context,
        providerId,
        canvas.width - 78,
        38,
        38,
        providerId === 'claude' || providerId === 'antigravity'
          ? palette.provider(providerId)
          : palette.text,
      );

      const cardTop = 112;
      context.fillStyle = palette.surface;
      context.beginPath();
      context.roundRect(28, cardTop, 664, Math.max(88, contentHeight + 16), 20);
      context.fill();
      let cursor = cardTop + 8;
      rows.forEach((row, index) => {
        if (index > 0) {
          context.fillStyle = palette.separator;
          context.fillRect(50, cursor, 620, 1);
        }
        if (row.kind === 'quota') {
          const remaining = Math.max(0, 100 - row.quota.usedPercent);
          context.fillStyle = palette.text;
          context.font = '600 19px system-ui';
          context.fillText(row.label, 52, cursor + 27);
          context.textAlign = 'right';
          context.font = '17px system-ui';
          context.fillStyle = palette.secondary;
          context.fillText(`${remaining.toFixed(0)}% left`, 668, cursor + 27);
          context.textAlign = 'left';
          context.fillStyle = palette.track;
          context.beginPath();
          context.roundRect(52, cursor + 42, 616, 7, 4);
          context.fill();
          context.fillStyle = palette.fill;
          const fillWidth = 616 * Math.min(1, Math.max(0, row.quota.usedPercent / 100));
          if (fillWidth > 0) {
            context.beginPath();
            context.roundRect(52, cursor + 42, fillWidth, 7, Math.min(4, fillWidth / 2));
            context.fill();
          }
          context.fillStyle = palette.text;
          context.font = '17px system-ui';
          context.fillText(`${row.quota.usedPercent.toFixed(0)}% used`, 52, cursor + 72);
          cursor += rowHeight(row);
        } else {
          context.fillStyle = palette.text;
          context.font = '600 18px system-ui';
          context.fillText(row.label, 52, cursor + 34);
          context.textAlign = 'right';
          context.fillStyle = palette.secondary;
          context.font = '17px system-ui';
          context.fillText(row.value, 668, cursor + 34);
          context.textAlign = 'left';
          cursor += rowHeight(row);
        }
      });
      context.fillStyle = palette.secondary;
      context.font = '15px system-ui';
      context.fillText('OpenQuota · Local usage snapshot', 42, canvas.height - 22);
      await copyCanvas(canvas, snapshot);
    } catch {
      settingsError = 'Provider screenshot could not be copied.';
    }
  }
  async function shareTotalSpend(projection: SpendProjection) {
    const card = document.querySelector<HTMLElement>('[data-total-spend]');
    if (!card) return false;
    try {
      const canvas = document.createElement('canvas');
      canvas.width = 720;
      canvas.height = 500;
      const context = canvas.getContext('2d');
      if (!context) throw new Error('Canvas unavailable');
      const palette = canvasPalette();
      const metric = settingsState.settings.totalSpendMetric;
      const period = settingsState.settings.totalSpendPeriod;
      const display = (value: number | null) => {
        if (value === null) return '—';
        if (metric === 'tokens')
          return new Intl.NumberFormat('en-US', {
            notation: 'compact',
            maximumFractionDigits: 1,
          }).format(value);
        return `$${value >= 100 ? value.toFixed(0) : value.toFixed(2)}`;
      };
      const metricTitle =
        metric === 'tokens' ? 'Tokens' : metric === 'costPerMillion' ? 'Cost/MTok' : 'Cost';
      const unit =
        metric === 'tokens' ? 'tokens' : metric === 'costPerMillion' ? '/ MTok' : 'total';
      context.fillStyle = palette.tray;
      context.fillRect(0, 0, canvas.width, canvas.height);
      context.fillStyle = palette.text;
      context.font = '600 30px system-ui';
      context.fillText('Total Spend', 38, 58);
      context.fillStyle = palette.secondary;
      context.font = '17px system-ui';
      context.fillText(`${metricTitle} · OpenQuota`, 38, 86);

      context.fillStyle = palette.surface;
      context.beginPath();
      context.roundRect(28, 112, 664, 332, 22);
      context.fill();

      const options = [
        { id: 'today', label: 'Today' },
        { id: 'yesterday', label: 'Yesterday' },
        { id: 'last30Days', label: '30 Days' },
      ];
      context.fillStyle = palette.track;
      context.beginPath();
      context.roundRect(48, 134, 624, 42, 21);
      context.fill();
      options.forEach((option, index) => {
        const left = 52 + index * 205;
        if (option.id === period) {
          context.fillStyle = palette.tray;
          context.beginPath();
          context.roundRect(left, 138, 197, 34, 17);
          context.fill();
        }
        context.fillStyle = option.id === period ? palette.text : palette.secondary;
        context.font = `${option.id === period ? '600' : '500'} 16px system-ui`;
        context.textAlign = 'center';
        context.fillText(option.label, left + 98, 161);
      });
      context.textAlign = 'left';

      if (projection.centerValue === null) {
        context.fillStyle = palette.secondary;
        context.font = '18px system-ui';
        context.textAlign = 'center';
        const message =
          metric === 'tokens'
            ? 'No token data for this period'
            : metric === 'costPerMillion'
              ? 'No cost-per-token data for this period'
              : 'No cost data for this period';
        context.fillText(message, 360, 305);
        context.textAlign = 'left';
      } else {
        const total = projection.slices.reduce((sum, slice) => sum + slice.value, 0);
        const floored = projection.slices.map((slice) =>
          Math.max(total > 0 ? slice.value / total : 0, 0.025),
        );
        const flooredTotal = floored.reduce((sum, share) => sum + share, 0);
        let start = -Math.PI / 2;
        projection.slices.forEach((slice, index) => {
          const width = (floored[index] / flooredTotal) * Math.PI * 2;
          const gap = Math.min(0.025, width * 0.15);
          context.beginPath();
          context.strokeStyle = palette.provider(slice.id);
          context.lineWidth = 34;
          context.lineCap = 'round';
          context.arc(184, 302, 76, start + gap, start + width - gap);
          context.stroke();
          start += width;
        });
        context.fillStyle = palette.text;
        context.font = '600 25px system-ui';
        context.textAlign = 'center';
        context.fillText(display(projection.centerValue), 184, 300);
        context.fillStyle = palette.secondary;
        context.font = '14px system-ui';
        context.fillText(unit, 184, 323);
        context.textAlign = 'left';

        projection.slices.forEach((slice, index) => {
          const y = 246 + index * 42;
          context.fillStyle = palette.provider(slice.id);
          context.beginPath();
          context.arc(335, y - 5, 6, 0, Math.PI * 2);
          context.fill();
          context.fillStyle = palette.text;
          context.font = '18px system-ui';
          context.fillText(providerDisplayName(slice.id), 352, y);
          context.fillStyle = palette.secondary;
          context.textAlign = 'right';
          context.fillText(display(slice.value), 652, y);
          context.textAlign = 'left';
        });
      }
      context.fillStyle = palette.secondary;
      context.font = '15px system-ui';
      context.fillText('OpenQuota · Local usage snapshot', 38, 478);
      await copyCanvas(canvas, card.innerText.trim());
      return true;
    } catch {
      settingsError = 'Total Spend screenshot could not be copied.';
      return false;
    }
  }
  async function copyDataPath() {
    try {
      const path = await invoke<string>('get_app_data_path');
      await navigator.clipboard.writeText(path);
      showConfirmation('Data path copied');
    } catch {
      settingsError = 'OpenQuota data path could not be copied.';
    }
  }
  function topBarTitle() {
    if (screen.startsWith('provider:')) return providerDisplayName(screen.slice(9));
    return screen === 'settings' ? 'Settings' : 'Customize';
  }
  function closeAboutFromBackdrop(event: MouseEvent) {
    if (event.target === event.currentTarget) showAbout = false;
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
    try {
      settingsState = await invoke<SettingsViewState>('request_notification_permission');
    } catch {
      settingsError = 'Notification permission could not be requested.';
    }
  }
  async function checkForUpdates(manual = false) {
    if (checkingUpdate || installingUpdate) return;
    checkingUpdate = true;
    if (manual) updateError = null;
    try {
      const status = await invoke<UpdateStatus>('check_for_updates');
      updateStatus = status;
      saveSettings({
        ...settingsState.settings,
        lastUpdateCheckAt: new Date().toISOString(),
      });
      if (manual && !status.available && screen !== 'settings') {
        showConfirmation(`OpenQuota ${status.currentVersion} is up to date.`);
      }
    } catch (error) {
      if (manual) {
        updateError = typeof error === 'string' ? error : 'Updates could not be checked.';
      }
    } finally {
      checkingUpdate = false;
    }
  }
  async function installUpdate() {
    if (installingUpdate || checkingUpdate) return;
    installingUpdate = true;
    updateProgress = { phase: 'downloading', downloaded: 0, total: null, percent: null };
    updateError = null;
    try {
      await invoke('install_update');
    } catch (error) {
      updateError = typeof error === 'string' ? error : 'The update could not be installed.';
      installingUpdate = false;
      updateProgress = null;
    }
  }
  async function openUpdatePage() {
    try {
      await invoke('open_update_page');
    } catch (error) {
      updateError =
        typeof error === 'string' ? error : 'The OpenQuota download page could not be opened.';
    }
  }
  function nextUpdateLabel(value: string | undefined) {
    if (!value) return 'Waiting for first update';
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return 'Next update unavailable';
    const seconds = Math.max(0, Math.ceil((date.getTime() + 300_000 - now) / 1000));
    return seconds >= 60
      ? `Next update in ${Math.ceil(seconds / 60)}m`
      : `Next update in ${seconds}s`;
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
      if (event.key === 'Escape') {
        event.preventDefault();
        if (showAbout) {
          showAbout = false;
          return;
        }
        back();
      } else if (event.key === 'Enter' && screen === 'dashboard') {
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
    const cleanup: Array<() => void> = [];
    void listen<UsageViewState>('usage-state', (event) => (viewState = event.payload)).then(
      (stop) => cleanup.push(stop),
    );
    void listen<SettingsViewState>(
      'settings-state',
      (event) => (settingsState = event.payload),
    ).then((stop) => cleanup.push(stop));
    void listen<string>('open-screen', (event) =>
      navigate(event.payload === 'settings' ? 'settings' : 'customize'),
    ).then((stop) => cleanup.push(stop));
    void listen('popup-hidden', () => {
      resetTransientUi();
      navigate('dashboard');
    }).then((stop) => cleanup.push(stop));
    void listen<UpdateProgress>('update-progress', (event) => {
      updateProgress = event.payload;
    }).then((stop) => cleanup.push(stop));
    void invoke<UsageViewState>('get_usage_state')
      .then((state) => (viewState = state))
      .catch(() => (settingsError = 'OpenQuota backend is unavailable.'));
    void invoke<SettingsViewState>('get_app_settings')
      .then((state) => {
        settingsState = state;
        automaticUpdatesReady = true;
      })
      .catch(() => (settingsError = 'Settings are unavailable.'));
    return () => {
      document.removeEventListener('keydown', handleKeydown);
      window.clearInterval(clock);
      window.cancelAnimationFrame(measureFrame);
      window.cancelAnimationFrame(resizeFrame);
      motionQuery.removeEventListener('change', updateMotionPreference);
      document.documentElement.removeAttribute('data-reduced-motion');
      mutationObserver.disconnect();
      resizeObserver?.disconnect();
      cleanup.forEach((stop) => stop());
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
          onclick={resetCustomization}
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
    {#if settingsError}<div class="notice notice--blocking" role="alert">{settingsError}</div>{/if}
    <div class="screen-stage">
      {#key screen}
        <div
          class="screen-page"
          data-screen={screen}
          in:horizontalPageTransition={{
            direction: slideDirection,
            duration: reducedMotion || !slidePageTransition ? 0 : 420,
            easing: springOut,
          }}
          out:horizontalPageTransition={{
            direction: -slideDirection,
            duration: reducedMotion || !slidePageTransition ? 0 : 420,
            easing: springOut,
          }}
        >
          {#if screen === 'dashboard'}
            <Dashboard
              {viewState}
              settings={settingsState.settings}
              {now}
              onSettingsChange={saveSettings}
              onCustomize={() => navigate('customize')}
              onOpenProviderCustomize={(id) => navigate(`provider:${id}`)}
              onShare={shareProvider}
              onShareTotal={shareTotalSpend}
              onRefresh={refresh}
              {reducedMotion}
              {updateStatus}
              {installingUpdate}
              {updateProgress}
              {updateError}
              onInstallUpdate={installUpdate}
              onOpenUpdatePage={openUpdatePage}
            />
          {:else if screen === 'settings'}
            <SettingsScreen
              settingsView={settingsState}
              onChange={saveSettings}
              onRequestNotifications={requestNotifications}
              {updateError}
              {checkingUpdate}
              {updateStatus}
              onCheckForUpdates={() => void checkForUpdates(true)}
              onCustomize={() => navigate('customize')}
              onCopyDataPath={copyDataPath}
            />
          {:else if screen === 'customize'}
            <CustomizeProviderList
              settings={settingsState.settings}
              onOpen={(id) => navigate(`provider:${id}`)}
              onChange={saveCustomization}
              onSettings={() => navigate('settings')}
            />
          {:else if screen.startsWith('provider:')}
            <CustomizeProviderDetail
              settings={settingsState.settings}
              providerId={screen.slice(9)}
              onChange={saveCustomization}
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
        aria-label="Refresh provider usage"
      >
        <span>OpenQuota 0.1.0</span><small
          >{anyRefreshing ? 'Updating…' : nextUpdateLabel(latestRefresh)}</small
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
                ><Icon name="share" /><span>Share Screenshot</span><Icon
                  name="chevron-right"
                  size={12}
                /></summary
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
          onclick={() => (showAbout = false)}>×</button
        >
        <OpenQuotaMark size={44} />
        <h1>OpenQuota</h1>
        <p>Version 0.1.0</p>
        <small>Private, local usage monitoring for your AI coding tools.</small>
      </div>
    </div>
  {/if}
</main>
