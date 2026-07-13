import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import type { ProviderViewState, SettingsViewState, UsageViewState } from './lib/types';
import {
  antigravityState,
  claudeState,
  codexState,
  liveState,
  settingsState,
} from './test/appFixtures';

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

type InvokeArgs = { settings?: SettingsViewState['settings'] };
type InvokeImplementation = (command: string, args?: InvokeArgs) => unknown;

function mockInvoke(implementation: InvokeImplementation) {
  mocks.invoke.mockImplementation((command: string, args?: InvokeArgs) => {
    if (command === 'get_bootstrap_state') {
      return Promise.all([
        implementation('get_usage_state', args),
        implementation('get_app_settings', args),
      ]).then(([usage, settings]) => ({ usage, settings }));
    }
    return implementation(command, args);
  });
}

describe('OpenQuota dashboard', () => {
  beforeEach(() => {
    mocks.currentMonitor.mockResolvedValue({
      scaleFactor: 1,
      workArea: { size: { width: 1280, height: 700 } },
    });
    mocks.listen.mockReset().mockResolvedValue(vi.fn());
    mocks.invoke.mockReset();
    mockInvoke((command: string, args?: InvokeArgs) => {
      if (
        command === 'get_usage_state' ||
        command === 'refresh_usage' ||
        command === 'refresh_provider_usage'
      )
        return Promise.resolve(liveState);
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      if (command === 'save_app_settings')
        return Promise.resolve({
          ...settingsState,
          settings: args?.settings ?? settingsState.settings,
        });
      if (command === 'request_notification_permission')
        return Promise.resolve({ ...settingsState, notificationPermission: 'granted' });
      if (command === 'open_notification_settings') return Promise.resolve();
      if (command === 'reset_customization') return Promise.resolve(settingsState);
      if (command === 'reset_provider_customization') return Promise.resolve(settingsState);
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
    });
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
    expect(screen.getByText(`OpenQuota ${import.meta.env.APP_VERSION}`)).toBeInTheDocument();
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
    mockInvoke((command: string) => {
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
    await fireEvent.click(screen.getByRole('combobox', { name: 'Total Spend Metric' }));
    await fireEvent.click(screen.getByRole('option', { name: 'Tokens' }));
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
    mockInvoke((command: string, args?: { settings?: SettingsViewState['settings'] }) => {
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
    });
    render(App);
    const totalSpend = await screen.findByRole('region', { name: 'Total Spend' });
    expect(within(totalSpend).getByText('No cost data for this period')).toBeInTheDocument();
    await fireEvent.click(within(totalSpend).getByRole('combobox', { name: 'Total Spend Metric' }));
    await fireEvent.click(screen.getByRole('option', { name: 'Tokens' }));
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
    const providerHeader = screen.getByRole('group', { name: 'Drag Codex to reorder' });
    expect(providerHeader).toHaveAttribute('data-reorder-handle');
    expect(providerHeader.closest('.provider-section')).toHaveAttribute(
      'data-reorder-group',
      'dashboard-providers',
    );
    expect(providerHeader).not.toHaveAttribute('draggable');
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

  it('resets one provider through the backend metric catalog', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Customize' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Customize codex' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Reset Codex' }));

    expect(mocks.invoke).toHaveBeenCalledWith('reset_provider_customization', {
      providerId: 'codex',
    });
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
    await fireEvent.click(screen.getByRole('combobox', { name: 'Density' }));
    await fireEvent.click(screen.getByRole('option', { name: 'Compact' }));
    await fireEvent.click(screen.getByRole('combobox', { name: 'Time Format' }));
    await fireEvent.click(screen.getByRole('option', { name: '24-hour' }));
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

  it('copies the real application data path', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Copy Path' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('C:\\OpenQuota\\Data'));
    expect(screen.getByRole('status')).toHaveTextContent('Data path copied');
  });

  it('shows the detected Linux fallback mode in Settings', async () => {
    mockInvoke((command: string) => {
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
    expect(screen.getByRole('checkbox', { name: /Almost Out/ })).toBeChecked();
  });

  it('offers system settings only when enabled notifications are blocked', async () => {
    mockInvoke((command: string, args?: { settings?: SettingsViewState['settings'] }) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings')
        return Promise.resolve({
          ...settingsState,
          notificationPermission: 'denied',
          settings: {
            ...settingsState.settings,
            notifications: { ...settingsState.settings.notifications, almostOut: true },
          },
        });
      if (command === 'save_app_settings')
        return Promise.resolve({
          ...settingsState,
          notificationPermission: 'denied',
          settings: args?.settings ?? settingsState.settings,
        });
      if (command === 'open_notification_settings') return Promise.resolve();
      if (command === 'resize_main_window') return Promise.resolve();
      return Promise.reject(new Error(`unexpected command ${command}`));
    });

    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByText('Notifications are blocked')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Open Settings' }));
    expect(mocks.invoke).toHaveBeenCalledWith('open_notification_settings');
  });

  it('preserves cached values and exposes a stale refresh error', async () => {
    mockInvoke((command: string) => {
      if (command === 'get_usage_state')
        return Promise.resolve({
          providers: {
            codex: {
              ...codexState,
              source: 'cache',
              stale: true,
              error: 'Could not connect to Codex.',
              errorKind: 'network',
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
    await fireEvent.click(screen.getByRole('button', { name: 'Refresh all provider usage' }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('refresh_usage'));
    await fireEvent.keyDown(document, { key: 'Escape' });
    expect(mocks.invoke).toHaveBeenCalledWith('dismiss_main_window');
  });

  it('refreshes only the provider selected in a context menu', async () => {
    let finishRefresh: ((state: UsageViewState) => void) | undefined;
    const refreshResult = new Promise<UsageViewState>((resolve) => (finishRefresh = resolve));
    const multiProviderState: UsageViewState = {
      providers: { codex: codexState, claude: claudeState },
      lastFullRefreshAt: new Date(Date.now() - 240_000).toISOString(),
    };
    const multiProviderSettings: SettingsViewState = {
      ...settingsState,
      settings: {
        ...settingsState.settings,
        showTotalSpend: false,
        providers: [
          ...settingsState.settings.providers,
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
            ],
          },
        ],
      },
    };
    mockInvoke((command: string) => {
      if (command === 'get_usage_state') return Promise.resolve(multiProviderState);
      if (command === 'get_app_settings') return Promise.resolve(multiProviderSettings);
      if (command === 'refresh_provider_usage') return refreshResult;
      if (command === 'resize_main_window') return Promise.resolve();
      return Promise.resolve();
    });

    render(App);
    await screen.findByRole('group', { name: 'Claude provider' });
    const codex = screen.getByRole('group', { name: 'Codex provider' });
    const claude = screen.getByRole('group', { name: 'Claude provider' });
    expect(screen.getByText('Next update in 1m')).toBeInTheDocument();
    await fireEvent.contextMenu(codex, {
      clientX: 120,
      clientY: 180,
    });
    await fireEvent.click(await screen.findByRole('menuitem', { name: 'Refresh Codex' }));

    expect(mocks.invoke).toHaveBeenCalledWith('refresh_provider_usage', { providerId: 'codex' });
    expect(within(codex).getByLabelText('Refreshing')).toBeInTheDocument();
    expect(within(claude).queryByLabelText('Refreshing')).not.toBeInTheDocument();
    expect(screen.getByText('Updating…')).toBeInTheDocument();

    finishRefresh?.(multiProviderState);
    await waitFor(() =>
      expect(within(codex).queryByLabelText('Refreshing')).not.toBeInTheDocument(),
    );
    expect(screen.getByText('Next update in 1m')).toBeInTheDocument();
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
    mockInvoke((command: string) => {
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

    await fireEvent.click(screen.getByRole('button', { name: 'Refresh all provider usage' }));
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

  it('keeps provider chrome and card alignment while initial Claude usage is loading', async () => {
    const pendingClaude: ProviderViewState = {
      source: 'none',
      refreshing: true,
      stale: false,
      error: null,
      errorKind: null,
      lastAttemptAt: null,
      snapshot: null,
    };
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
                id: 'claude.weekly',
                enabled: true,
                section: 'alwaysVisible',
                pinned: true,
              },
            ],
          },
        ],
      },
    };
    mockInvoke((command: string) => {
      if (command === 'get_usage_state')
        return Promise.resolve({ providers: { claude: pendingClaude } });
      if (command === 'get_app_settings') return Promise.resolve(claudeSettings);
      if (command === 'resize_main_window') return Promise.resolve();
      return new Promise(() => undefined);
    });

    render(App);
    const provider = await screen.findByRole('group', { name: 'Claude provider' });
    const card = within(provider).getByRole('region', { name: 'Claude usage' });

    expect(within(provider).getByRole('heading', { name: 'Claude' })).toBeInTheDocument();
    expect(within(provider).getByLabelText('Refreshing')).toBeInTheDocument();
    expect(within(card).getByText('Reading Claude usage…')).toBeInTheDocument();
    expect(card).toHaveClass('provider-card');
    expect(within(card).getByText('Reading Claude usage…')).toHaveClass('empty-row');
  });

  it('restores stable provider chrome when a refresh request fails to start', async () => {
    mockInvoke((command: string) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      if (command === 'refresh_usage') return Promise.reject(new Error('offline'));
      return Promise.resolve();
    });
    render(App);
    const provider = await screen.findByRole('group', { name: 'Codex provider' });
    const card = within(provider).getByRole('region', { name: 'Codex usage' });
    await fireEvent.click(screen.getByRole('button', { name: 'Refresh all provider usage' }));
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
    const close = screen.getByRole('button', { name: 'Close About' });
    expect(close.querySelector('svg')).not.toBeNull();
    expect(close).not.toHaveTextContent('×');
    await fireEvent.click(close);
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

  it('lets a dropdown consume Escape without navigating away from Settings', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    const theme = screen.getByRole('combobox', { name: 'Theme' });

    await fireEvent.keyDown(theme, { key: 'ArrowDown' });
    expect(screen.getByRole('listbox', { name: 'Theme' })).toBeInTheDocument();
    await fireEvent.keyDown(document.activeElement!, { key: 'Escape' });

    expect(screen.queryByRole('listbox', { name: 'Theme' })).not.toBeInTheDocument();
    expect(theme).toHaveFocus();
    expect(screen.getByRole('heading', { name: 'Settings' })).toBeInTheDocument();
  });
});
