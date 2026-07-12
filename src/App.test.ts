import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import type {
  ProviderViewState,
  SettingsViewState,
  UpdateProgress,
  UsageViewState,
} from './lib/types';

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  currentMonitor: vi.fn(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
vi.mock('@tauri-apps/api/window', () => ({
  currentMonitor: mocks.currentMonitor,
  getCurrentWindow: () => ({
    scaleFactor: () => Promise.resolve(1),
    innerSize: () => Promise.resolve({ width: 320, height: 600 }),
  }),
}));

const codexState: ProviderViewState = {
  source: 'live',
  refreshing: false,
  stale: false,
  error: null,
  lastAttemptAt: null,
  snapshot: {
    providerId: 'codex',
    plan: 'Plus',
    refreshedAt: '2026-07-10T10:00:00Z',
    warnings: [],
    quotas: [
      {
        id: 'session',
        label: 'Session',
        usedPercent: 32,
        resetsAt: '2099-01-01T00:00:00Z',
        periodSeconds: 18000,
        format: 'percent',
        usedValue: null,
        limitValue: null,
      },
      {
        id: 'weekly',
        label: 'Weekly',
        usedPercent: 59,
        resetsAt: '2099-01-07T00:00:00Z',
        periodSeconds: 604800,
        format: 'percent',
        usedValue: null,
        limitValue: null,
      },
    ],
    usage: {
      today: { tokens: 2100000, estimatedCostUsd: 3.84, estimateComplete: true },
      yesterday: { tokens: 684000, estimatedCostUsd: 1.27, estimateComplete: true },
      last30Days: { tokens: 3000000, estimatedCostUsd: 5.11, estimateComplete: true },
      daily: [
        { date: '2026-07-10', tokens: 2100000, estimatedCostUsd: 3.84, estimateComplete: true },
      ],
      unknownModels: [],
    },
  },
};
const liveState: UsageViewState = { providers: { codex: codexState } };

const claudeState: ProviderViewState = {
  source: 'live',
  refreshing: false,
  stale: false,
  error: null,
  lastAttemptAt: null,
  snapshot: {
    providerId: 'claude',
    plan: 'Max',
    refreshedAt: '2026-07-11T10:00:00Z',
    warnings: [],
    quotas: [
      {
        id: 'session',
        label: 'Session',
        usedPercent: 20,
        resetsAt: '2099-01-01T00:00:00Z',
        periodSeconds: 18000,
        format: 'percent',
        usedValue: null,
        limitValue: null,
      },
      {
        id: 'extra',
        label: 'Extra Usage',
        usedPercent: 25,
        resetsAt: null,
        periodSeconds: 0,
        format: 'dollars',
        usedValue: 12.5,
        limitValue: 50,
      },
    ],
    usage: { today: null, yesterday: null, last30Days: null, daily: [], unknownModels: [] },
  },
};

const antigravityState: ProviderViewState = {
  source: 'live',
  refreshing: false,
  stale: false,
  error: null,
  lastAttemptAt: null,
  snapshot: {
    providerId: 'antigravity',
    plan: 'Pro',
    refreshedAt: '2026-07-11T10:00:00Z',
    warnings: [],
    quotas: [
      {
        id: 'geminiPro',
        label: 'Session',
        usedPercent: 0,
        resetsAt: '2099-01-01T00:00:00Z',
        periodSeconds: 18000,
        format: 'percent',
        usedValue: null,
        limitValue: null,
      },
      {
        id: 'geminiWeekly',
        label: 'Weekly',
        usedPercent: 13,
        resetsAt: '2099-01-07T00:00:00Z',
        periodSeconds: 604800,
        format: 'percent',
        usedValue: null,
        limitValue: null,
      },
    ],
    usage: { today: null, yesterday: null, last30Days: null, daily: [], unknownModels: [] },
  },
};

const settingsState: SettingsViewState = {
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
    detectionNoticeDismissed: true,
    providers: [
      {
        id: 'codex',
        enabled: true,
        detected: true,
        expanded: false,
        metrics: [
          { id: 'codex.session', enabled: true, section: 'alwaysVisible', pinned: true },
          { id: 'codex.weekly', enabled: true, section: 'alwaysVisible', pinned: true },
          { id: 'codex.trend', enabled: true, section: 'alwaysVisible', pinned: false },
          { id: 'codex.today', enabled: true, section: 'onDemand', pinned: false },
          { id: 'codex.yesterday', enabled: true, section: 'onDemand', pinned: false },
          { id: 'codex.last30', enabled: true, section: 'onDemand', pinned: false },
        ],
      },
    ],
  },
};

describe('OpenQuota dashboard', () => {
  beforeEach(() => {
    mocks.currentMonitor.mockResolvedValue({
      scaleFactor: 1,
      workArea: { size: { width: 1280, height: 700 } },
    });
    mocks.listen.mockReset().mockResolvedValue(vi.fn());
    mocks.invoke
      .mockReset()
      .mockImplementation(
        (command: string, args?: { settings?: SettingsViewState['settings'] }) => {
          if (command === 'get_usage_state' || command === 'refresh_usage')
            return Promise.resolve(liveState);
          if (command === 'get_app_settings') return Promise.resolve(settingsState);
          if (command === 'save_app_settings')
            return Promise.resolve({
              ...settingsState,
              settings: args?.settings ?? settingsState.settings,
            });
          if (command === 'request_notification_permission')
            return Promise.resolve({ ...settingsState, notificationPermission: 'granted' });
          if (command === 'reset_customization') return Promise.resolve(settingsState);
          if (command === 'resize_main_window') return Promise.resolve();
          if (command === 'get_app_data_path') return Promise.resolve('C:\\OpenQuota\\Data');
          if (command === 'dismiss_main_window') return Promise.resolve();
          if (command === 'check_for_updates')
            return Promise.resolve({
              available: false,
              currentVersion: '0.1.0',
              version: null,
              body: null,
              installable: true,
              releaseUrl: 'https://github.com/deviffyy/OpenQuota/releases/latest',
            });
          return Promise.reject(new Error(`unexpected command ${command}`));
        },
      );
  });
  afterEach(cleanup);

  it('renders quota, total spend, and the 30-day trend from backend data', async () => {
    render(App);
    expect(await screen.findByText('Plus')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Session used' })).toHaveAttribute(
      'aria-valuenow',
      '32',
    );
    expect(screen.getByRole('progressbar', { name: 'Weekly used' })).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Total Spend' })).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Usage Trend' })).toBeInTheDocument();
    expect(screen.getByText('OpenQuota 0.1.1')).toBeInTheDocument();
  });

  it('renders Claude and Antigravity independently with provider-specific quota formats', async () => {
    const multiProviderSettings = {
      ...settingsState,
      settings: {
        ...settingsState.settings,
        providers: [
          {
            id: 'claude',
            enabled: true,
            detected: true,
            expanded: false,
            metrics: [
              {
                id: 'claude.session',
                enabled: true,
                section: 'alwaysVisible' as const,
                pinned: true,
              },
              {
                id: 'claude.extra',
                enabled: true,
                section: 'alwaysVisible' as const,
                pinned: false,
              },
            ],
          },
          ...settingsState.settings.providers,
          {
            id: 'antigravity',
            enabled: true,
            detected: true,
            expanded: false,
            metrics: [
              {
                id: 'antigravity.geminiPro',
                enabled: true,
                section: 'alwaysVisible' as const,
                pinned: true,
              },
              {
                id: 'antigravity.geminiWeekly',
                enabled: true,
                section: 'alwaysVisible' as const,
                pinned: true,
              },
            ],
          },
        ],
      },
    };
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_usage_state')
        return Promise.resolve({
          providers: { claude: claudeState, codex: codexState, antigravity: antigravityState },
        });
      if (command === 'get_app_settings') return Promise.resolve(multiProviderSettings);
      if (command === 'check_for_updates')
        return Promise.resolve({
          available: false,
          currentVersion: '0.1.0',
          version: null,
          body: null,
          installable: true,
          releaseUrl: 'https://github.com/deviffyy/OpenQuota/releases/latest',
        });
      return Promise.resolve(multiProviderSettings);
    });

    render(App);
    expect(await screen.findByRole('heading', { name: 'Claude' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Antigravity' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '$37.50 left' })).toBeInTheDocument();
    expect(screen.getAllByRole('progressbar')).toHaveLength(6);
    expect(
      within(screen.getByRole('region', { name: 'Total Spend' })).getByRole('img', {
        name: 'Only includes Claude and Codex',
      }),
    ).toBeInTheDocument();
  });

  it('persists Total Spend metric and period choices', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.change(screen.getByLabelText('Total Spend Metric'), {
      target: { value: 'tokens' },
    });
    await fireEvent.click(screen.getByRole('button', { name: '30 Days' }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({
          settings: expect.objectContaining({ totalSpendPeriod: 'last30Days' }),
        }),
      ),
    );
  });

  it('explains unavailable cost and reveals measured tokens for the same period', async () => {
    mocks.invoke.mockImplementation(
      (command: string, args?: { settings?: SettingsViewState['settings'] }) => {
        if (command === 'get_usage_state')
          return Promise.resolve({
            providers: {
              codex: {
                ...codexState,
                snapshot: {
                  ...codexState.snapshot!,
                  usage: {
                    ...codexState.snapshot!.usage,
                    today: {
                      tokens: 2_100_000,
                      estimatedCostUsd: null,
                      estimateComplete: false,
                    },
                  },
                },
              },
            },
          });
        if (command === 'get_app_settings') return Promise.resolve(settingsState);
        if (command === 'save_app_settings')
          return Promise.resolve({
            ...settingsState,
            settings: args?.settings ?? settingsState.settings,
          });
        return Promise.resolve(liveState);
      },
    );
    render(App);
    const totalSpend = await screen.findByRole('region', { name: 'Total Spend' });
    expect(within(totalSpend).getByText('No cost data for this period')).toBeInTheDocument();
    await fireEvent.change(within(totalSpend).getByLabelText('Total Spend Metric'), {
      target: { value: 'tokens' },
    });
    expect(within(totalSpend).getByText('Codex')).toBeInTheDocument();
    expect(within(totalSpend).getByText('2.1')).toBeInTheDocument();
    expect(within(totalSpend).getByText('million')).toBeInTheDocument();
    expect(within(totalSpend).getByText('2.1M')).toBeInTheDocument();
    expect(within(totalSpend).queryByText('No data')).not.toBeInTheDocument();
  });

  it('reveals On Demand metrics without losing their saved order', async () => {
    render(App);
    await screen.findByText('Plus');
    expect(screen.queryByText('$3.84 · 2.1M tokens')).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
    expect(screen.getByText('$3.84 · 2.1M tokens')).toBeInTheDocument();
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith('save_app_settings', expect.any(Object)),
    );
  });

  it('uses the compact reference caret instead of a labeled On Demand divider', async () => {
    render(App);
    const toggle = await screen.findByRole('button', { name: 'Show more' });
    expect(screen.getByRole('group', { name: 'Drag Codex to reorder' })).toHaveAttribute(
      'draggable',
      'true',
    );
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(toggle).not.toHaveTextContent('On Demand');
    await fireEvent.click(toggle);
    expect(screen.getByRole('button', { name: 'Show less' })).toHaveAttribute(
      'aria-expanded',
      'true',
    );
  });

  it('renders the Total Spend ring as rounded SVG sectors', async () => {
    render(App);
    expect(await screen.findByRole('region', { name: 'Total Spend' })).toBeInTheDocument();
    await waitFor(() => expect(document.querySelector('.spend-ring svg')).not.toBeNull());
    expect(document.querySelector('.spend-ring__segment')).not.toBeNull();
    expect(document.querySelector('.period-switcher__selection')).not.toBeNull();
  });

  it('opens Customize and exposes the two-section metric layout', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Customize' }));
    expect(screen.getByRole('heading', { name: 'Customize' })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Customize codex' }));
    expect(screen.getByRole('group', { name: 'Always Visible metrics' })).toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'On Demand metrics' })).toBeInTheDocument();
  });

  it('enforces the two-pinned-metrics limit in Customize', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Customize' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Customize codex' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Pin Today' }));
    expect(screen.getByText('Up to 2 stars per provider')).toBeInTheDocument();
  });

  it('persists Used/Left changes made directly from a quota row', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByRole('button', { name: '68% left' }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({ settings: expect.objectContaining({ usageDisplay: 'used' }) }),
      ),
    );
  });

  it('persists compact density from Settings', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    await fireEvent.change(screen.getByLabelText('Density'), { target: { value: 'compact' } });
    await fireEvent.change(screen.getByLabelText('Time Format'), {
      target: { value: 'twentyFourHour' },
    });
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({ settings: expect.objectContaining({ density: 'compact' }) }),
      ),
    );
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({
          settings: expect.objectContaining({ timeFormat: 'twentyFourHour' }),
        }),
      ),
    );
  });

  it('surfaces local-only privacy and copies the real application data path', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByText('Share Anonymous Usage')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Copy Path' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('C:\\OpenQuota\\Data'));
    expect(screen.getByRole('status')).toHaveTextContent('Data path copied');
  });

  it('shows the detected Linux fallback mode in Settings', async () => {
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings')
        return Promise.resolve({
          ...settingsState,
          standaloneWindow: true,
          platformSummary: 'GNOME · Wayland · standalone window',
        });
      if (command === 'check_for_updates')
        return Promise.resolve({
          available: false,
          currentVersion: '0.1.0',
          version: null,
          body: null,
          installable: true,
          releaseUrl: 'https://github.com/deviffyy/OpenQuota/releases/latest',
        });
      return Promise.resolve();
    });
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByText('GNOME · Wayland · standalone window')).toBeInTheDocument();
  });

  it('checks for updates manually and persists the macOS menu bar style', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Check for Updates…' }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('check_for_updates'));
    expect(await screen.findByText('OpenQuota 0.1.0 is up to date.')).toBeInTheDocument();
    expect(mocks.invoke).toHaveBeenCalledWith(
      'save_app_settings',
      expect.objectContaining({
        settings: expect.objectContaining({ lastUpdateCheckAt: expect.any(String) }),
      }),
    );
    await fireEvent.change(screen.getByRole('combobox', { name: /Icon Style/ }), {
      target: { value: 'bars' },
    });
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({ settings: expect.objectContaining({ menuBarStyle: 'bars' }) }),
      ),
    );
  });

  it('surfaces an available update on the dashboard and allows it to be dismissed', async () => {
    mocks.invoke.mockImplementation(
      (command: string, args?: { settings?: SettingsViewState['settings'] }) => {
        if (command === 'get_usage_state') return Promise.resolve(liveState);
        if (command === 'get_app_settings') return Promise.resolve(settingsState);
        if (command === 'save_app_settings')
          return Promise.resolve({
            ...settingsState,
            settings: args?.settings ?? settingsState.settings,
          });
        if (command === 'check_for_updates')
          return Promise.resolve({
            available: true,
            currentVersion: '0.1.0',
            version: '0.2.0',
            body: 'New release',
            installable: true,
            releaseUrl: 'https://github.com/deviffyy/OpenQuota/releases/latest',
          });
        return Promise.resolve();
      },
    );
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Check for Updates…' }));
    expect(await screen.findByRole('region', { name: 'Update Available' })).toHaveTextContent(
      'OpenQuota 0.2.0 is ready to download.',
    );
    expect(screen.getByText('New release')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByRole('region', { name: 'Update Available' })).not.toBeInTheDocument();
    expect(mocks.invoke).toHaveBeenCalledWith(
      'save_app_settings',
      expect.objectContaining({
        settings: expect.objectContaining({ dismissedUpdateVersion: '0.2.0' }),
      }),
    );
  });

  it('opens the package download page when in-app installation is unavailable', async () => {
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      if (command === 'save_app_settings') return Promise.resolve(settingsState);
      if (command === 'check_for_updates')
        return Promise.resolve({
          available: true,
          currentVersion: '0.1.0',
          version: '0.2.0',
          body: null,
          installable: false,
          releaseUrl: 'https://github.com/deviffyy/OpenQuota/releases/latest',
        });
      if (command === 'open_update_page') return Promise.resolve();
      return Promise.resolve();
    });
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Check for Updates…' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Download Update' }));
    expect(mocks.invoke).toHaveBeenCalledWith('open_update_page');
  });

  it('installs supported updates and renders native download progress', async () => {
    let progressListener: ((event: { payload: UpdateProgress }) => void) | undefined;
    mocks.listen.mockImplementation(
      (event: string, callback: (event: { payload: UpdateProgress }) => void) => {
        if (event === 'update-progress') progressListener = callback;
        return Promise.resolve(vi.fn());
      },
    );
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      if (command === 'save_app_settings') return Promise.resolve(settingsState);
      if (command === 'check_for_updates')
        return Promise.resolve({
          available: true,
          currentVersion: '0.1.0',
          version: '0.2.0',
          body: 'Safer updates',
          installable: true,
          releaseUrl: 'https://github.com/deviffyy/OpenQuota/releases/latest',
        });
      if (command === 'install_update') return Promise.resolve();
      return Promise.resolve();
    });

    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Check for Updates…' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Install Update' }));
    expect(mocks.invoke).toHaveBeenCalledWith('install_update');

    progressListener?.({
      payload: { phase: 'downloading', downloaded: 42, total: 100, percent: 42 },
    });
    expect(await screen.findByText('Downloading update… 42%')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Update download' })).toHaveAttribute(
      'aria-valuenow',
      '42',
    );

    progressListener?.({
      payload: { phase: 'installing', downloaded: 100, total: 100, percent: 100 },
    });
    expect(await screen.findByText('Installing update…')).toBeInTheDocument();
  });

  it('records a global shortcut and requests notification permission', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    const recorder = screen.getByRole('button', { name: 'Record Shortcut' });
    await fireEvent.click(recorder);
    await fireEvent.keyDown(recorder, { key: 'Q', code: 'KeyQ', ctrlKey: true, shiftKey: true });
    await fireEvent.click(screen.getByRole('checkbox', { name: /Almost Out/ }));
    await waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({
          settings: expect.objectContaining({ globalShortcut: 'Ctrl+Shift+Q' }),
        }),
      );
      expect(mocks.invoke).toHaveBeenCalledWith('request_notification_permission');
    });
  });

  it('preserves cached values and exposes a stale refresh error', async () => {
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_usage_state')
        return Promise.resolve({
          providers: {
            codex: {
              ...codexState,
              source: 'cache',
              stale: true,
              error: 'Could not connect to Codex.',
            },
          },
        });
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      return Promise.resolve(liveState);
    });
    render(App);
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not connect to Codex.');
    expect(screen.getByText('Outdated')).toHaveAttribute(
      'data-tooltip',
      expect.stringMatching(/^Last updated/),
    );
  });

  it('supports manual refresh and popup close shortcuts', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByRole('button', { name: 'Refresh provider usage' }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('refresh_usage'));
    await fireEvent.keyDown(document, { key: 'Escape' });
    expect(mocks.invoke).toHaveBeenCalledWith('dismiss_main_window');
  });

  it('keeps the Claude card structure stable while optional quota data refreshes', async () => {
    let finishRefresh: ((state: UsageViewState) => void) | undefined;
    const refreshResult = new Promise<UsageViewState>((resolve) => (finishRefresh = resolve));
    const initialClaude: ProviderViewState = {
      ...claudeState,
      snapshot: {
        ...claudeState.snapshot!,
        quotas: claudeState.snapshot!.quotas.filter((quota) => quota.id !== 'extra'),
      },
    };
    const initialState: UsageViewState = { providers: { claude: initialClaude } };
    const refreshedState: UsageViewState = { providers: { claude: claudeState } };
    const claudeSettings: SettingsViewState = {
      ...settingsState,
      settings: {
        ...settingsState.settings,
        showTotalSpend: false,
        providers: [
          {
            id: 'claude',
            enabled: true,
            detected: true,
            expanded: false,
            metrics: [
              {
                id: 'claude.session',
                enabled: true,
                section: 'alwaysVisible',
                pinned: true,
              },
              {
                id: 'claude.extra',
                enabled: true,
                section: 'alwaysVisible',
                pinned: false,
              },
            ],
          },
        ],
      },
    };
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_usage_state') return Promise.resolve(initialState);
      if (command === 'get_app_settings') return Promise.resolve(claudeSettings);
      if (command === 'refresh_usage') return refreshResult;
      return Promise.resolve();
    });

    render(App);
    const provider = await screen.findByRole('group', { name: 'Claude provider' });
    const card = within(provider).getByRole('region', { name: 'Claude usage' });
    const extraRow = within(provider).getByRole('group', { name: 'Extra Usage options' });
    expect(within(extraRow).getByText('No data')).toBeInTheDocument();
    const statusSlot = provider.querySelector('.provider-status-slot');
    expect(statusSlot).toBeInTheDocument();
    expect(statusSlot).not.toHaveClass('active');

    await fireEvent.click(screen.getByRole('button', { name: 'Refresh provider usage' }));
    expect(await within(provider).findByLabelText('Refreshing')).toBeInTheDocument();
    expect(statusSlot).toHaveClass('active');
    expect(within(provider).getByRole('region', { name: 'Claude usage' })).toBe(card);
    expect(within(provider).getByRole('group', { name: 'Extra Usage options' })).toBe(extraRow);

    finishRefresh?.(refreshedState);
    await waitFor(() => expect(within(extraRow).queryByText('No data')).not.toBeInTheDocument());
    expect(within(provider).getByRole('region', { name: 'Claude usage' })).toBe(card);
    expect(within(provider).getByRole('group', { name: 'Extra Usage options' })).toBe(extraRow);
    expect(statusSlot).not.toHaveClass('active');
  });

  it('restores stable provider chrome when a refresh request fails to start', async () => {
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      if (command === 'refresh_usage') return Promise.reject(new Error('offline'));
      return Promise.resolve();
    });
    render(App);
    const provider = await screen.findByRole('group', { name: 'Codex provider' });
    const card = within(provider).getByRole('region', { name: 'Codex usage' });
    await fireEvent.click(screen.getByRole('button', { name: 'Refresh provider usage' }));
    await waitFor(() =>
      expect(within(provider).queryByLabelText('Refreshing')).not.toBeInTheDocument(),
    );
    expect(within(provider).getByRole('region', { name: 'Codex usage' })).toBe(card);
    expect(provider.querySelector('.provider-status-slot')).not.toHaveClass('active');
    expect(screen.getByText('OpenQuota could not start a provider refresh.')).toBeInTheDocument();
  });

  it('shows platform-correct Ctrl shortcuts and handles Ctrl+Q', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByText('Options').closest('summary')!);
    expect(screen.getByText('Ctrl+,')).toBeInTheDocument();
    expect(screen.getByText('Ctrl+Q')).toBeInTheDocument();

    await fireEvent.keyDown(document, { key: 'q', ctrlKey: true });
    expect(mocks.invoke).toHaveBeenCalledWith('quit_app');
  });

  it('closes the custom Options surface after a command like a native menu', async () => {
    render(App);
    await screen.findByText('Plus');
    const summary = screen.getByText('Options').closest('summary')!;
    const menu = summary.closest('details')!;
    await fireEvent.click(summary);
    expect(menu).toHaveAttribute('open');
    await fireEvent.click(screen.getByRole('button', { name: 'Check for Updates…' }));
    expect(menu).not.toHaveAttribute('open');
  });

  it('honors Reduce Motion and refits the panel when On Demand changes content height', async () => {
    const originalMatchMedia = window.matchMedia;
    window.matchMedia = vi.fn().mockReturnValue({
      matches: true,
      media: '(prefers-reduced-motion: reduce)',
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    });
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    });
    try {
      render(App);
      await waitFor(() => expect(document.documentElement).toHaveAttribute('data-reduced-motion'));
      const resizeCalls = () =>
        mocks.invoke.mock.calls.filter(([command]) => command === 'resize_main_window');
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith('resize_main_window', { height: 200 }),
      );
      const callsBeforeExpand = resizeCalls().length;
      await fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
      await waitFor(() => expect(resizeCalls().length).toBeGreaterThan(callsBeforeExpand));
    } finally {
      delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
      window.matchMedia = originalMatchMedia;
    }
  });

  it('suppresses the WebView context menu outside custom menu targets', async () => {
    render(App);
    await screen.findByText('Plus');
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    screen.getByLabelText('OpenQuota usage dashboard').dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
  });

  it('opens and dismisses the About panel from Options', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'About OpenQuota' }));
    expect(screen.getByRole('dialog', { name: 'About OpenQuota' })).toBeInTheDocument();
    await fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('dialog', { name: 'About OpenQuota' })).not.toBeInTheDocument();
  });

  it('matches provider context-menu and Customize to Settings navigation behavior', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.contextMenu(screen.getByRole('group', { name: 'Codex provider' }), {
      clientX: 120,
      clientY: 180,
    });
    expect(screen.getByRole('menuitem', { name: 'Share Screenshot' })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Customize…' }));
    expect(screen.getByRole('heading', { name: 'Codex' })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Back' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByRole('heading', { name: 'Settings' })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Customize' }));
    expect(screen.getByRole('heading', { name: 'Customize' })).toBeInTheDocument();
  });

  it('supports native-like keyboard navigation in dashboard context menus', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.contextMenu(screen.getByRole('group', { name: 'Codex provider' }), {
      clientX: 120,
      clientY: 180,
    });
    const hide = screen.getByRole('menuitem', { name: 'Hide Codex' });
    await waitFor(() => expect(hide).toHaveFocus());
    await fireEvent.keyDown(hide, { key: 'ArrowDown' });
    expect(screen.getByRole('menuitem', { name: 'Refresh Codex' })).toHaveFocus();
    await fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
    expect(document.querySelector('.context-menu')).toBeNull();
  });

  it('undoes the latest customization with Ctrl+Z', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Customize' }));
    const toggle = screen.getByRole('checkbox', { name: 'Enable codex' });
    await fireEvent.click(toggle);
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({
          settings: expect.objectContaining({
            providers: expect.arrayContaining([
              expect.objectContaining({ id: 'codex', enabled: false }),
            ]),
          }),
        }),
      ),
    );
    await fireEvent.keyDown(document, { key: 'z', ctrlKey: true });
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenLastCalledWith(
        'save_app_settings',
        expect.objectContaining({
          settings: expect.objectContaining({
            providers: expect.arrayContaining([
              expect.objectContaining({ id: 'codex', enabled: true }),
            ]),
          }),
        }),
      ),
    );
  });

  it('reorders dashboard metrics directly with a custom drag lift', async () => {
    render(App);
    await screen.findByText('Plus');
    const session = screen.getByRole('group', { name: 'Session options' });
    const weekly = screen.getByRole('group', { name: 'Weekly options' });
    const dataTransfer = { effectAllowed: 'none', setDragImage: vi.fn() };
    await fireEvent.dragStart(session, { dataTransfer });
    await fireEvent.dragOver(weekly, { dataTransfer });
    await fireEvent.drop(weekly, { dataTransfer });
    expect(dataTransfer.setDragImage).toHaveBeenCalled();
    await waitFor(() =>
      expect(mocks.invoke.mock.calls.some(([command]) => command === 'save_app_settings')).toBe(
        true,
      ),
    );
    const saveCall = [...mocks.invoke.mock.calls]
      .reverse()
      .find(([command]) => command === 'save_app_settings');
    const saved = saveCall?.[1] as { settings: SettingsViewState['settings'] };
    expect(
      saved.settings.providers
        .find((provider) => provider.id === 'codex')
        ?.metrics.map((metric) => metric.id),
    ).toEqual([
      'codex.weekly',
      'codex.session',
      'codex.trend',
      'codex.today',
      'codex.yesterday',
      'codex.last30',
    ]);
  });
});
