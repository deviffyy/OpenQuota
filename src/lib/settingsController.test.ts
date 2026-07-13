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
    notificationPermission: 'prompt',
    integrationError: null,
    standaloneWindow: false,
    platformSummary: null,
    settings: {
      schemaVersion: 4,
      providers: [],
      knownProviderIds: [],
      showTotalSpend: true,
      theme,
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
});
