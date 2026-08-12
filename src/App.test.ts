import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import type {
  AppSettings,
  ProviderCatalog,
  ProviderViewState,
  SettingsViewState,
  UsageViewState,
} from './lib/types';
import {
  antigravityState,
  claudeState,
  codexState,
  liveState,
  providerCatalog,
  settingsState,
} from './test/appFixtures';

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  currentMonitor: vi.fn(),
  startDragging: vi.fn(),
  startResizeDragging: vi.fn(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
vi.mock('@tauri-apps/api/window', () => ({
  currentMonitor: mocks.currentMonitor,
  getCurrentWindow: () => ({
    scaleFactor: () => Promise.resolve(1),
    innerSize: () => Promise.resolve({ width: 320, height: 600 }),
    startDragging: mocks.startDragging,
    startResizeDragging: mocks.startResizeDragging,
  }),
}));

type InvokeArgs = {
  settings?: SettingsViewState['settings'];
  providerId?: string;
  linkIndex?: number;
  height?: number;
};
type InvokeImplementation = (command: string, args?: InvokeArgs) => unknown;

function mockInvoke(
  implementation: InvokeImplementation,
  catalog: ProviderCatalog = providerCatalog,
) {
  mocks.invoke.mockImplementation((command: string, args?: InvokeArgs) => {
    if (command === 'get_bootstrap_state') {
      return Promise.all([
        implementation('get_usage_state', args),
        implementation('get_app_settings', args),
      ]).then(([usage, settings]) => ({ usage, settings, catalog }));
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
    mocks.startDragging.mockReset().mockResolvedValue(undefined);
    mocks.startResizeDragging.mockReset().mockResolvedValue(undefined);
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
      if (command === 'open_provider_link') return Promise.resolve();
      if (command === 'reset_customization') return Promise.resolve(settingsState);
      if (command === 'reset_provider_customization') return Promise.resolve(settingsState);
      if (command === 'get_panel_resize_edge') return Promise.resolve('bottom');
      if (command === 'get_panel_height_mode') return Promise.resolve('automatic');
      if (command === 'fit_panel_to_content') return Promise.resolve(true);
      if (command === 'set_panel_height_automatic') return Promise.resolve();
      if (command === 'set_panel_height_manual') return Promise.resolve();
      if (command === 'begin_panel_resize') return Promise.resolve('bottom');
      if (command === 'lock_panel_resize_axis') return Promise.resolve();
      if (command === 'get_log_path') return Promise.resolve('C:\\OpenQuota\\logs\\OpenQuota.log');
      if (command === 'open_log_folder') return Promise.resolve();
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
    const { container } = render(App);
    expect(await screen.findByText('Plus')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Session used' })).toHaveAttribute(
      'aria-valuenow',
      '32',
    );
    expect(screen.getByRole('progressbar', { name: 'Weekly used' })).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Total Spend' })).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Usage Trend' })).toBeInTheDocument();
    expect(container.querySelector('.spend-ring__label')).toHaveAttribute(
      'data-tooltip',
      '$3.84 · Estimated locally, so it may be off',
    );
    expect(screen.getByText(`OpenQuota ${import.meta.env.APP_VERSION}`)).toBeInTheDocument();
    expect(container.querySelector('.floating-chrome')).not.toBeInTheDocument();
  });

  it('toggles floating window mode from the footer pin when a tray is available', async () => {
    render(App);
    await screen.findByText('Plus');

    const pin = screen.getByRole('button', { name: 'Keep Window Open' });
    expect(pin).toHaveAttribute('aria-pressed', 'false');
    await fireEvent.click(pin);

    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({
          settings: expect.objectContaining({ windowMode: 'floating' }),
        }),
      ),
    );
    expect(screen.getByRole('button', { name: 'Return to Tray Popup' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Return to Tray Popup' }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({
          settings: expect.objectContaining({ windowMode: 'popup' }),
        }),
      ),
    );
  });

  it('provides a native drag region and hide control in floating window mode', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    });
    const userAgent = navigator.userAgent;
    Object.defineProperty(navigator, 'userAgent', {
      configurable: true,
      value: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
    });
    const defaultInvoke = mocks.invoke.getMockImplementation()!;
    mockInvoke((command: string, args?: InvokeArgs) => {
      if (command === 'get_app_settings')
        return Promise.resolve({
          ...settingsState,
          settings: { ...settingsState.settings, windowMode: 'floating' as const },
        });
      if (command === 'get_panel_resize_edge') return new Promise(() => undefined);
      return defaultInvoke(command, args);
    });

    try {
      const { container } = render(App);
      await screen.findByText('Plus');
      const dragSurface = container.querySelector<HTMLElement>('.floating-chrome__drag');
      expect(dragSurface).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Hide OpenQuota' })).toBeInTheDocument();
      expect(screen.getByRole('separator', { name: 'Resize panel height' })).toHaveClass(
        'panel-resize-dragger--bottom',
      );

      expect(dragSurface).toHaveAttribute('data-tauri-drag-region');
    } finally {
      delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
      Object.defineProperty(navigator, 'userAgent', { configurable: true, value: userAgent });
    }
  });

  it('refreshes the resize edge after switching from floating window to tray popup', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    });
    const userAgent = navigator.userAgent;
    Object.defineProperty(navigator, 'userAgent', {
      configurable: true,
      value: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
    });
    let appliedMode: AppSettings['windowMode'] = 'floating';
    const defaultInvoke = mocks.invoke.getMockImplementation()!;
    mockInvoke((command: string, args?: InvokeArgs) => {
      if (command === 'get_app_settings')
        return Promise.resolve({
          ...settingsState,
          settings: { ...settingsState.settings, windowMode: 'floating' as const },
        });
      if (command === 'save_app_settings') {
        appliedMode = args?.settings?.windowMode ?? appliedMode;
        return Promise.resolve({
          ...settingsState,
          settings: args?.settings ?? settingsState.settings,
        });
      }
      if (command === 'get_panel_resize_edge')
        return Promise.resolve(appliedMode === 'floating' ? 'bottom' : 'top');
      return defaultInvoke(command, args);
    });

    try {
      render(App);
      await screen.findByText('Plus');
      expect(screen.getByRole('separator', { name: 'Resize panel height' })).toHaveClass(
        'panel-resize-dragger--bottom',
      );
      await fireEvent.click(screen.getByLabelText('Open options'));
      await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
      await fireEvent.click(screen.getByRole('combobox', { name: 'Window Mode' }));
      await fireEvent.click(screen.getByRole('option', { name: 'Tray Popup' }));

      await waitFor(() =>
        expect(screen.getByRole('separator', { name: 'Resize panel height' })).toHaveClass(
          'panel-resize-dragger--top',
        ),
      );
    } finally {
      delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
      Object.defineProperty(navigator, 'userAgent', { configurable: true, value: userAgent });
    }
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

  it('renames an observed Claude card from its context menu', async () => {
    const claudeSettings: SettingsViewState = {
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
              { id: 'claude.session', enabled: true, section: 'alwaysVisible', pinned: true },
            ],
          },
        ],
      },
    };
    const claudeUsage: UsageViewState = { providers: { claude: claudeState } };
    mockInvoke((command: string, args?: InvokeArgs) => {
      if (
        command === 'get_usage_state' ||
        command === 'refresh_usage' ||
        command === 'refresh_provider_usage'
      )
        return Promise.resolve(claudeUsage);
      if (command === 'get_app_settings') return Promise.resolve(claudeSettings);
      if (command === 'save_app_settings')
        return Promise.resolve({
          ...claudeSettings,
          settings: args?.settings ?? claudeSettings.settings,
        });
      if (command === 'get_panel_resize_edge') return Promise.resolve('bottom');
      if (command === 'get_panel_height_mode') return Promise.resolve('automatic');
      if (command === 'fit_panel_to_content') return Promise.resolve(true);
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
    const provider = await screen.findByRole('group', { name: 'Claude provider' });
    await fireEvent.contextMenu(provider);
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Rename…' }));
    const dialog = screen.getByRole('dialog', { name: 'Rename Card' });
    const input = within(dialog).getByRole('textbox', { name: 'Name' });
    await fireEvent.input(input, { target: { value: 'Personal' } });
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Rename' }));

    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith('save_app_settings', {
        expectedAccountRevision: 0,
        settings: expect.objectContaining({ providerNames: { claude: 'Personal' } }),
      }),
    );
    expect(await screen.findByRole('heading', { name: 'Personal' })).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Move Personal' })).toHaveFocus(),
    );
    const savesAfterRename = mocks.invoke.mock.calls.filter(
      ([command]) => command === 'save_app_settings',
    ).length;
    await fireEvent.keyDown(document, { key: 'z', ctrlKey: true });
    await Promise.resolve();
    expect(
      mocks.invoke.mock.calls.filter(([command]) => command === 'save_app_settings'),
    ).toHaveLength(savesAfterRename);
  });

  it('renames an observed extra Claude account card', async () => {
    const providerId = 'claude@1234abcd';
    const extraCatalog = structuredClone(providerCatalog);
    const baseDefinition = extraCatalog.providers.find((provider) => provider.id === 'claude')!;
    extraCatalog.providers.push({
      ...baseDefinition,
      id: providerId,
      displayName: 'Claude — Work',
      fallbackEnabled: false,
      metrics: baseDefinition.metrics.map((metric) => ({
        ...metric,
        id: metric.id.replace('claude.', `${providerId}.`),
      })),
    });
    const extraSettings: SettingsViewState = {
      ...settingsState,
      renamableProviderIds: [providerId],
      settings: {
        ...settingsState.settings,
        providers: [
          {
            id: providerId,
            enabled: true,
            detected: true,
            expanded: false,
            metrics: [
              {
                id: `${providerId}.session`,
                enabled: true,
                section: 'alwaysVisible',
                pinned: true,
              },
            ],
          },
        ],
      },
    };
    const extraState: ProviderViewState = {
      ...claudeState,
      snapshot: claudeState.snapshot
        ? { ...claudeState.snapshot, providerId }
        : claudeState.snapshot,
    };
    const usage: UsageViewState = { providers: { [providerId]: extraState } };
    mockInvoke((command: string, args?: InvokeArgs) => {
      if (command === 'get_usage_state') return Promise.resolve(usage);
      if (command === 'get_app_settings') return Promise.resolve(extraSettings);
      if (command === 'save_app_settings')
        return Promise.resolve({
          ...extraSettings,
          settings: args?.settings ?? extraSettings.settings,
        });
      if (command === 'get_panel_resize_edge') return Promise.resolve('bottom');
      if (command === 'get_panel_height_mode') return Promise.resolve('automatic');
      if (command === 'fit_panel_to_content') return Promise.resolve(true);
      return Promise.resolve();
    }, extraCatalog);

    render(App);
    const provider = await screen.findByRole('group', { name: 'Claude — Work provider' });
    await fireEvent.contextMenu(provider);
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Rename…' }));
    const dialog = screen.getByRole('dialog', { name: 'Rename Card' });
    await fireEvent.input(within(dialog).getByRole('textbox', { name: 'Name' }), {
      target: { value: 'Client' },
    });
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Rename' }));

    expect(await screen.findByRole('heading', { name: 'Client' })).toBeInTheDocument();
    expect(mocks.invoke).toHaveBeenCalledWith('save_app_settings', {
      expectedAccountRevision: 0,
      settings: expect.objectContaining({ providerNames: { [providerId]: 'Client' } }),
    });
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
                    costEstimated: true,
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

  it('keeps neighboring provider values mounted while Codex On Demand morphs', async () => {
    const multiUsage: UsageViewState = {
      providers: { claude: claudeState, codex: codexState },
    };
    const multiSettings: SettingsViewState = {
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
              { id: 'claude.session', enabled: true, section: 'alwaysVisible', pinned: true },
            ],
          },
          settingsState.settings.providers[0],
        ],
      },
    };
    mockInvoke((command: string, args?: InvokeArgs) => {
      if (command === 'get_usage_state') return Promise.resolve(multiUsage);
      if (command === 'get_app_settings') return Promise.resolve(multiSettings);
      if (command === 'save_app_settings')
        return Promise.resolve({
          ...multiSettings,
          settings: args?.settings ?? multiSettings.settings,
        });
      return Promise.resolve();
    });

    render(App);
    const claude = await screen.findByRole('group', { name: 'Claude provider' });
    const codex = screen.getByRole('group', { name: 'Codex provider' });
    const claudeReading = within(claude).getByText('80% left');

    await fireEvent.click(within(codex).getByRole('button', { name: 'Show more' }));

    expect(claudeReading.isConnected).toBe(true);
    expect(within(claude).getByText('80% left')).toBe(claudeReading);
    expect(claude.closest('.provider-reorder-shell')).toHaveClass(
      'provider-reorder-shell--content-morph',
    );
    expect(codex.closest('.provider-reorder-shell')).toHaveClass(
      'provider-reorder-shell--content-morph',
    );
    for (const metric of claude.querySelectorAll('.metric-context-target')) {
      expect(metric).toHaveClass('metric-context-target--content-morph');
    }
  });

  it('uses the compact caret instead of a labeled On Demand divider', async () => {
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
    expect(screen.queryByRole('button', { name: 'Status, opens in browser' })).toBeNull();
    await fireEvent.click(toggle);
    expect(screen.getByRole('button', { name: 'Show less' })).toHaveAttribute(
      'aria-expanded',
      'true',
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Status, opens in browser' }));
    expect(mocks.invoke).toHaveBeenCalledWith('open_provider_link', {
      providerId: 'codex',
      linkIndex: 0,
    });
    expect(screen.getByRole('button', { name: 'Dashboard, opens in browser' })).toBeInTheDocument();
  });

  it('keeps the expander for a provider whose only expanded content is quick links', async () => {
    const linksOnlySettings = structuredClone(settingsState);
    linksOnlySettings.settings.providers[0].metrics =
      linksOnlySettings.settings.providers[0].metrics.map((metric) =>
        metric.section === 'onDemand' ? { ...metric, enabled: false } : metric,
      );
    mockInvoke((command: string, args?: InvokeArgs) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings') return Promise.resolve(linksOnlySettings);
      if (command === 'save_app_settings')
        return Promise.resolve({
          ...linksOnlySettings,
          settings: args?.settings ?? linksOnlySettings.settings,
        });
      return Promise.resolve();
    });

    render(App);
    const toggle = await screen.findByRole('button', { name: 'Show more' });
    expect(screen.queryByRole('button', { name: 'Status, opens in browser' })).toBeNull();

    await fireEvent.click(toggle);

    expect(screen.getByRole('button', { name: 'Status, opens in browser' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Dashboard, opens in browser' })).toBeInTheDocument();
  });

  it('renders the Total Spend ring as separated rounded SVG sectors', async () => {
    render(App);
    expect(await screen.findByRole('region', { name: 'Total Spend' })).toBeInTheDocument();
    await waitFor(() => expect(document.querySelector('.spend-ring svg')).not.toBeNull());
    const segment = document.querySelector('.spend-ring__segment');
    expect(segment?.tagName).toBe('path');
    expect(segment?.getAttribute('d')).toMatch(/^M .* A .* Q .* Z$/);
    expect(document.querySelector('.spend-ring__track')).toBeNull();
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

  it('persists the log level and exposes only the backend-owned log location', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    await fireEvent.click(screen.getByRole('combobox', { name: 'Log Level' }));
    await fireEvent.click(screen.getByRole('option', { name: 'Debug' }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({ settings: expect.objectContaining({ logLevel: 'debug' }) }),
      ),
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Copy Log Path' }));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith('C:\\OpenQuota\\logs\\OpenQuota.log'),
    );
    expect(mocks.invoke).toHaveBeenCalledWith('get_log_path');
    expect(screen.getByRole('status')).toHaveTextContent('Log path copied');

    const headings = screen
      .getAllByRole('heading', { level: 2 })
      .map((heading) => heading.textContent?.trim());
    expect(headings.indexOf('Advanced')).toBeLessThan(headings.indexOf('Updates'));
    expect(headings).not.toContain('Data');
    expect(screen.queryByText('Application Data')).not.toBeInTheDocument();

    await fireEvent.click(
      screen.getByRole('button', {
        name: /Reveal in Finder|Reveal in File Explorer|Open Containing Folder/,
      }),
    );
    expect(mocks.invoke).toHaveBeenCalledWith('open_log_folder');
  });

  it('shows log action failures inside the Advanced card', async () => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
    });
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));

    mocks.invoke.mockRejectedValueOnce(new Error('log path unavailable'));
    await fireEvent.click(screen.getByRole('button', { name: 'Copy Log Path' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      "Couldn't copy the log path to the clipboard.",
    );
  });

  it('shows the detected Linux fallback mode in Settings', async () => {
    mockInvoke((command: string) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings')
        return Promise.resolve({
          ...settingsState,
          trayAvailable: false,
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
    expect(screen.getByRole('button', { name: 'Close OpenQuota' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Keep Window Open' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Return to Tray Popup' })).not.toBeInTheDocument();
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByText('GNOME · Wayland · standalone window')).toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'Window Mode' })).not.toBeInTheDocument();
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

  it('lets reset details consume Escape before the popup shortcut', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Rate Limit Resets: 2 available' }));
    await fireEvent.click(screen.getAllByRole('button', { name: /Use reset expiring/ })[0]);

    const cancel = screen.getByRole('button', { name: 'Cancel' });
    cancel.focus();
    await fireEvent.keyDown(cancel, { key: 'Escape' });

    expect(screen.queryByText('Use this reset?')).not.toBeInTheDocument();
    const dialog = screen.getByRole('dialog', { name: 'Rate Limit Resets details' });
    expect(dialog).toHaveFocus();
    expect(mocks.invoke).not.toHaveBeenCalledWith('dismiss_main_window');

    await fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(
      screen.queryByRole('dialog', { name: 'Rate Limit Resets details' }),
    ).not.toBeInTheDocument();
    expect(mocks.invoke).not.toHaveBeenCalledWith('dismiss_main_window');
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
      return new Promise(() => undefined);
    });

    render(App);
    const provider = await screen.findByRole('group', { name: 'Claude provider' });
    const card = within(provider).getByRole('region', { name: 'Claude usage' });

    expect(within(provider).getByRole('heading', { name: 'Claude' })).toBeInTheDocument();
    expect(within(provider).getByLabelText('Refreshing')).toBeInTheDocument();
    expect(card).toHaveClass('provider-card');
    expect(card).toHaveAttribute('aria-busy', 'true');
    const session = within(card).getByRole('group', { name: 'Session options' });
    const weekly = within(card).getByRole('group', { name: 'Weekly options' });
    expect(within(session).getByText('No data')).toBeInTheDocument();
    expect(within(weekly).getByText('No data')).toBeInTheDocument();
    expect(within(card).queryByText('Reading Claude usage…')).toBeNull();
    const toggle = within(card).getByRole('button', { name: 'Show more' });
    expect(within(card).queryByRole('button', { name: 'Status, opens in browser' })).toBeNull();

    await fireEvent.click(toggle);

    expect(
      within(card).getByRole('button', { name: 'Status, opens in browser' }),
    ).toBeInTheDocument();
  });

  it('shows configured metric rows before a provider has produced any state', async () => {
    mockInvoke((command: string) => {
      if (command === 'get_usage_state') return Promise.resolve({ providers: {} });
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      return Promise.resolve();
    });

    render(App);
    const provider = await screen.findByRole('group', { name: 'Codex provider' });
    const card = within(provider).getByRole('region', { name: 'Codex usage' });

    expect(within(provider).queryByLabelText('Refreshing')).toBeNull();
    expect(card).not.toHaveAttribute('aria-busy');
    expect(
      within(within(card).getByRole('group', { name: 'Session options' })).getByText('No data'),
    ).toBeInTheDocument();
    expect(
      within(within(card).getByRole('group', { name: 'Weekly options' })).getByText('No data'),
    ).toBeInTheDocument();
  });

  it('keeps a snapshot-less provider error visible alongside its no-data metric rows', async () => {
    const failedCodex: ProviderViewState = {
      source: 'none',
      refreshing: false,
      stale: false,
      error: 'Sign in to Codex to load usage.',
      errorKind: 'authentication',
      lastAttemptAt: new Date().toISOString(),
      snapshot: null,
    };
    mockInvoke((command: string) => {
      if (command === 'get_usage_state')
        return Promise.resolve({ providers: { codex: failedCodex } });
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      return Promise.resolve();
    });

    render(App);
    const provider = await screen.findByRole('group', { name: 'Codex provider' });
    const card = within(provider).getByRole('region', { name: 'Codex usage' });

    expect(within(provider).getByRole('alert')).toHaveAttribute(
      'aria-label',
      'Sign in to Codex to load usage.',
    );
    expect(provider.querySelector('.provider-status-slot')).toHaveClass('active');
    expect(
      within(within(card).getByRole('group', { name: 'Session options' })).getByText('No data'),
    ).toBeInTheDocument();
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

  it('resets native Options and Share details when the popup is hidden', async () => {
    let emitMainWindowHidden: (() => void) | undefined;
    mocks.listen.mockImplementation(
      (eventName: string, handler: (event: { payload: unknown }) => void) => {
        if (eventName === 'main-window-hidden') {
          emitMainWindowHidden = () => handler({ payload: undefined });
        }
        return Promise.resolve(vi.fn());
      },
    );

    render(App);
    await screen.findByText('Plus');
    const optionsSummary = screen.getByLabelText('Open options');
    const optionsMenu = optionsSummary.closest('details')!;
    await fireEvent.click(optionsSummary);
    const shareSummary = screen.getByText('Share Screenshot').closest('summary')!;
    const shareMenu = shareSummary.closest('details')!;
    await fireEvent.click(shareSummary);
    expect(optionsMenu).toHaveAttribute('open');
    expect(shareMenu).toHaveAttribute('open');
    await waitFor(() =>
      expect(within(shareMenu).getByRole('button', { name: 'Codex' })).toBeInTheDocument(),
    );

    await waitFor(() => expect(emitMainWindowHidden).toBeTypeOf('function'));
    emitMainWindowHidden!();

    await waitFor(() => {
      expect(optionsMenu).not.toHaveAttribute('open');
      expect(shareMenu).not.toHaveAttribute('open');
      expect(within(shareMenu).queryByRole('button')).not.toBeInTheDocument();
    });

    await fireEvent.click(optionsSummary);
    expect(optionsMenu).toHaveAttribute('open');
    expect(shareMenu).not.toHaveAttribute('open');
    await fireEvent.click(shareSummary);
    expect(shareMenu).toHaveAttribute('open');
    await waitFor(() =>
      expect(within(shareMenu).getByRole('button', { name: 'Codex' })).toBeInTheDocument(),
    );
  });

  it('honors Reduce Motion without overriding a manually sized native panel', async () => {
    const originalMatchMedia = window.matchMedia;
    const defaultInvoke = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args?: InvokeArgs) => {
      if (command === 'get_panel_height_mode') return Promise.resolve('manual');
      return defaultInvoke(command, args);
    });
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
      await screen.findByText('Plus');
      await fireEvent.click(screen.getByLabelText('Open options'));
      await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
      await waitFor(() =>
        expect(screen.getByRole('combobox', { name: 'Panel Height' })).toHaveTextContent('Manual'),
      );
      await fireEvent.click(screen.getByLabelText('Back'));
      mocks.invoke.mockClear();
      await fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
      await new Promise((resolve) => setTimeout(resolve, 20));
      expect(mocks.invoke).not.toHaveBeenCalledWith('fit_panel_to_content', expect.anything());
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

  it('keeps native persistence active through synthetic pointer handoff events', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    });
    try {
      render(App);
      const grip = await screen.findByRole('separator', { name: 'Resize panel height' });
      await waitFor(() => expect(grip).toHaveClass('panel-resize-dragger--bottom'));

      await fireEvent.pointerDown(grip, { button: 0 });
      expect(mocks.invoke).toHaveBeenCalledWith('begin_panel_resize');
      await waitFor(() => expect(mocks.startResizeDragging).toHaveBeenCalledWith('South'));
      await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('lock_panel_resize_axis'));
      expect(mocks.invoke).not.toHaveBeenCalledWith('finish_panel_resize');

      await fireEvent.pointerCancel(window);
      await fireEvent.pointerUp(window);
      expect(mocks.invoke).not.toHaveBeenCalledWith('finish_panel_resize');
    } finally {
      delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    }
  });

  it('changes panel height mode from Settings Appearance and keeps grip double-click available', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    });
    const defaultInvoke = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args?: InvokeArgs) => {
      if (command === 'get_panel_height_mode') return Promise.resolve('manual');
      return defaultInvoke(command, args);
    });
    try {
      render(App);
      await screen.findByText('Plus');
      await fireEvent.click(screen.getByLabelText('Open options'));
      await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
      const windowMode = screen.getByRole('combobox', { name: 'Window Mode' });
      await fireEvent.click(windowMode);
      await fireEvent.click(screen.getByRole('option', { name: 'Floating Window' }));
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith(
          'save_app_settings',
          expect.objectContaining({
            settings: expect.objectContaining({ windowMode: 'floating' }),
          }),
        ),
      );

      const heightMode = screen.getByRole('combobox', { name: 'Panel Height' });
      await waitFor(() => expect(heightMode).toHaveTextContent('Manual'));

      await fireEvent.click(heightMode);
      await fireEvent.click(screen.getByRole('option', { name: 'Automatic' }));
      expect(mocks.invoke).toHaveBeenCalledWith('set_panel_height_automatic');
      await waitFor(() => expect(heightMode).toHaveTextContent('Automatic'));

      await fireEvent.click(heightMode);
      await fireEvent.click(screen.getByRole('option', { name: 'Manual' }));
      expect(mocks.invoke).toHaveBeenCalledWith('set_panel_height_manual');
      await waitFor(() => expect(heightMode).toHaveTextContent('Manual'));

      await fireEvent.click(screen.getByLabelText('Back'));
      const grip = screen.getByRole('separator', { name: 'Resize panel height' });
      await fireEvent.pointerDown(grip, { button: 0, detail: 1 });
      await fireEvent.pointerDown(grip, { button: 0, detail: 2 });
      await waitFor(() =>
        expect(
          mocks.invoke.mock.calls.filter(([command]) => command === 'set_panel_height_automatic'),
        ).toHaveLength(2),
      );
    } finally {
      delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    }
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

  it('does not preselect a context-menu item after a pointer invocation', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.contextMenu(screen.getByRole('group', { name: 'Codex provider' }), {
      button: 2,
      clientX: 120,
      clientY: 180,
    });
    const hide = screen.getByRole('menuitem', { name: 'Hide Codex' });
    const menu = hide.closest<HTMLElement>('[role="menu"]');
    if (!menu) throw new Error('Context menu was not rendered.');
    await waitFor(() => expect(menu).toHaveFocus());
    expect(hide).not.toHaveFocus();

    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(hide).toHaveFocus();
  });

  it('hides a dashboard metric without removing its menu bar star', async () => {
    render(App);
    await screen.findByText('Plus');
    await fireEvent.contextMenu(screen.getByRole('group', { name: 'Session options' }), {
      clientX: 120,
      clientY: 180,
    });
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Hide' }));

    await waitFor(() => {
      const save = [...mocks.invoke.mock.calls]
        .reverse()
        .find((call: unknown[]) => call[0] === 'save_app_settings');
      const settings = save?.[1]?.settings as AppSettings | undefined;
      const session = settings?.providers
        .find((provider) => provider.id === 'codex')
        ?.metrics.find((metric) => metric.id === 'codex.session');
      expect(session).toMatchObject({ enabled: false, pinned: true });
    });
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
