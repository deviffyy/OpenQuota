import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import type { ProviderViewState, SettingsViewState, UsageViewState } from './lib/types';

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));

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
    schemaVersion: 3,
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
          if (command === 'get_app_data_path') return Promise.resolve('C:\\OpenQuota\\Data');
          if (command === 'dismiss_main_window') return Promise.resolve();
          if (command === 'check_for_updates')
            return Promise.resolve({
              available: false,
              currentVersion: '0.1.0',
              version: null,
              body: null,
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
    await fireEvent.change(screen.getByLabelText('Total Spend metric'), {
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
    await fireEvent.click(
      await within(totalSpend).findByRole('button', { name: 'View available tokens' }),
    );
    expect(within(totalSpend).getByText('Codex')).toBeInTheDocument();
    expect(within(totalSpend).getAllByText('2.1M')).toHaveLength(2);
    expect(within(totalSpend).queryByText('No data')).not.toBeInTheDocument();
  });

  it('reveals On Demand metrics without losing their saved order', async () => {
    render(App);
    await screen.findByText('Plus');
    expect(screen.queryByText('$3.84 · 2.1M tokens')).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: /On Demand/ }));
    expect(screen.getByText('$3.84 · 2.1M tokens')).toBeInTheDocument();
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith('save_app_settings', expect.any(Object)),
    );
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
    expect(screen.getByText('Up to 2 pinned metrics per provider')).toBeInTheDocument();
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
    expect(screen.getByText('Anonymous Usage')).toBeInTheDocument();
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
        });
      return Promise.resolve();
    });
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByText('GNOME · Wayland · standalone window')).toBeInTheDocument();
  });

  it('checks for updates on startup and persists the macOS menu bar style', async () => {
    render(App);
    await screen.findByText('Plus');
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('check_for_updates'));
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    await fireEvent.change(screen.getByRole('combobox', { name: /Menu Bar/ }), {
      target: { value: 'bars' },
    });
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({ settings: expect.objectContaining({ menuBarStyle: 'bars' }) }),
      ),
    );
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
    expect(screen.getByText('Stale')).toBeInTheDocument();
  });

  it('supports manual refresh and popup close shortcuts', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByRole('button', { name: 'Refresh provider usage' }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('refresh_usage'));
    await fireEvent.keyDown(document, { key: 'Escape' });
    expect(mocks.invoke).toHaveBeenCalledWith('dismiss_main_window');
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
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Customize' }));
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
    const customize = screen.getByRole('menuitem', { name: 'Customize' });
    await waitFor(() => expect(customize).toHaveFocus());
    await fireEvent.keyDown(customize, { key: 'ArrowDown' });
    expect(screen.getByRole('menuitem', { name: 'Share Screenshot' })).toHaveFocus();
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
