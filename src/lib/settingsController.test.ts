import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppSettings, SettingsViewState } from './types';
import { SettingsController } from './settingsController.svelte';

const mocks = vi.hoisted(() => ({
  getAppSettings: vi.fn(),
  saveAppSettings: vi.fn(),
}));

vi.mock('./backend', () => mocks);

function settingsView(theme: AppSettings['theme'] = 'system'): SettingsViewState {
  return {
    accountRevision: 0,
    renamableProviderIds: [],
    notificationPermission: 'prompt',
    integrationError: null,
    trayAvailable: true,
    platformSummary: null,
    settings: {
      schemaVersion: 6,
      language: 'en',
      providerNames: {},
      providers: [],
      knownProviderIds: [],
      showTotalSpend: true,
      theme,
      density: 'default',
      windowMode: 'popup',
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
      logLevel: 'info',
      notifications: { almostOut: false, cuttingItClose: false, willRunOut: false },
      totalSpendMetric: 'cost',
      totalSpendPeriod: 'today',
      detectionNoticeDismissed: false,
    },
  };
}

describe('SettingsController', () => {
  beforeEach(() => {
    mocks.getAppSettings.mockReset();
    mocks.saveAppSettings.mockReset();
  });

  it('serializes saves and keeps the latest optimistic revision', async () => {
    const resolvers: Array<(state: SettingsViewState) => void> = [];
    mocks.saveAppSettings.mockImplementation(
      (settings: AppSettings) =>
        new Promise<SettingsViewState>((resolve) => {
          resolvers.push(() => resolve({ ...settingsView(), settings }));
        }),
    );
    const controller = new SettingsController(vi.fn());
    controller.setState(settingsView());

    controller.save({ ...settingsView().settings, theme: 'light' });
    controller.save({ ...settingsView().settings, theme: 'dark' });

    expect(controller.state?.settings.theme).toBe('dark');
    await vi.waitFor(() => expect(mocks.saveAppSettings).toHaveBeenCalledTimes(1));
    expect(mocks.saveAppSettings).toHaveBeenCalledWith(expect.any(Object), 0);
    resolvers[0](settingsView('light'));
    await vi.waitFor(() => expect(mocks.saveAppSettings).toHaveBeenCalledTimes(2));
    resolvers[1](settingsView('dark'));
    await vi.waitFor(() => expect(controller.state?.settings.theme).toBe('dark'));
  });

  it('reloads persisted state after the latest save fails', async () => {
    const onError = vi.fn();
    mocks.saveAppSettings.mockRejectedValue('Autostart unavailable.');
    mocks.getAppSettings.mockResolvedValue(settingsView('system'));
    const controller = new SettingsController(onError);
    controller.setState(settingsView('light'));

    controller.save({ ...settingsView().settings, theme: 'dark' });

    await vi.waitFor(() => expect(controller.state?.settings.theme).toBe('system'));
    expect(onError).toHaveBeenCalledWith('Autostart unavailable.');
  });

  it('reloads an external account change after pending saves settle', async () => {
    let finishSave: ((state: SettingsViewState) => void) | undefined;
    mocks.saveAppSettings.mockImplementation(
      () => new Promise<SettingsViewState>((resolve) => (finishSave = resolve)),
    );
    const external = {
      ...settingsView('dark'),
      accountRevision: 1,
    };
    mocks.getAppSettings.mockResolvedValue(external);
    const controller = new SettingsController(vi.fn());
    controller.setState(settingsView());

    controller.save({ ...settingsView().settings, theme: 'light' });
    await vi.waitFor(() => expect(mocks.saveAppSettings).toHaveBeenCalledTimes(1));
    controller.acceptExternalState(external);
    expect(controller.state?.accountRevision).toBe(0);
    finishSave?.(settingsView('light'));

    await vi.waitFor(() => expect(mocks.getAppSettings).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(controller.state?.accountRevision).toBe(1));
    expect(controller.state?.settings.theme).toBe('dark');
  });

  it('never replaces a newer account state with an older response', () => {
    const controller = new SettingsController(vi.fn());
    controller.setState({ ...settingsView('dark'), accountRevision: 2 });

    controller.setState({ ...settingsView('light'), accountRevision: 1 });
    controller.acceptExternalState({ ...settingsView('light'), accountRevision: 1 });

    expect(controller.state?.accountRevision).toBe(2);
    expect(controller.state?.settings.theme).toBe('dark');
  });
});
