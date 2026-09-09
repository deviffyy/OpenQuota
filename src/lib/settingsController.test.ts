import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppSettings, SettingsViewState } from './types';
import { SettingsController } from './settingsController.svelte';

const mocks = vi.hoisted(() => ({
  getAppSettings: vi.fn(),
  saveAppSettings: vi.fn(),
}));

vi.mock('./backend', () => mocks);

function settingsView(
  theme: AppSettings['theme'] = 'system',
  settingsRevision = 0,
  accountRevision = 0,
): SettingsViewState {
  return {
    resolvedLanguage: 'en',
    settingsRevision,
    accountRevision,
    renamableProviderIds: [],
    notificationPermission: 'prompt',
    integrationError: null,
    trayAvailable: true,
    platformSummary: null,
    settings: {
      schemaVersion: 7,
      language: 'en',
      providerNames: {},
      providers: [],
      knownProviderIds: [],
      showTotalSpend: true,
      theme,
      density: 'default',
      reduceAnimations: false,
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
      (settings: AppSettings, expectedSettingsRevision: number) =>
        new Promise<SettingsViewState>((resolve) => {
          resolvers.push(() =>
            resolve({ ...settingsView('system', expectedSettingsRevision + 1), settings }),
          );
        }),
    );
    const controller = new SettingsController(vi.fn());
    controller.setState(settingsView());

    const firstSave = controller.save({ ...settingsView().settings, theme: 'light' });
    const secondSave = controller.save({ ...settingsView().settings, theme: 'dark' });

    expect(controller.state?.settings.theme).toBe('dark');
    await vi.waitFor(() => expect(mocks.saveAppSettings).toHaveBeenCalledTimes(1));
    expect(mocks.saveAppSettings).toHaveBeenCalledWith(expect.any(Object), 0, 0);
    resolvers[0](settingsView('light', 1));
    await vi.waitFor(() => expect(mocks.saveAppSettings).toHaveBeenCalledTimes(2));
    expect(mocks.saveAppSettings).toHaveBeenLastCalledWith(expect.any(Object), 1, 0);
    resolvers[1](settingsView('dark', 2));
    await Promise.all([firstSave, secondSave]);
    expect(controller.state?.settings.theme).toBe('dark');
    expect(controller.state?.settingsRevision).toBe(2);
  });

  it('reloads persisted state after the latest save fails', async () => {
    const onError = vi.fn();
    mocks.saveAppSettings.mockRejectedValue('Autostart unavailable.');
    mocks.getAppSettings.mockResolvedValue(settingsView('system'));
    const controller = new SettingsController(onError);
    controller.setState(settingsView('light'));

    controller.save({ ...settingsView().settings, theme: 'dark' });

    await vi.waitFor(() => expect(controller.state?.settings.theme).toBe('system'));
    expect(onError).toHaveBeenCalledWith('Settings could not be saved.');
  });

  it('reloads an external account change after pending saves settle', async () => {
    let finishSave: ((state: SettingsViewState) => void) | undefined;
    mocks.saveAppSettings.mockImplementation(
      () => new Promise<SettingsViewState>((resolve) => (finishSave = resolve)),
    );
    const external = {
      ...settingsView('dark', 2, 1),
    };
    mocks.getAppSettings.mockResolvedValue(external);
    const controller = new SettingsController(vi.fn());
    controller.setState(settingsView());

    controller.save({ ...settingsView().settings, theme: 'light' });
    await vi.waitFor(() => expect(mocks.saveAppSettings).toHaveBeenCalledTimes(1));
    controller.acceptExternalState(external);
    expect(controller.state?.accountRevision).toBe(0);
    finishSave?.(settingsView('light', 1));

    await vi.waitFor(() => expect(mocks.getAppSettings).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(controller.state?.accountRevision).toBe(1));
    expect(controller.state?.settings.theme).toBe('dark');
  });

  it('applies the newest queued event even when the follow-up refresh fails', async () => {
    let finishSave: ((state: SettingsViewState) => void) | undefined;
    mocks.saveAppSettings.mockImplementation(
      () => new Promise<SettingsViewState>((resolve) => (finishSave = resolve)),
    );
    mocks.getAppSettings.mockRejectedValue(new Error('offline'));
    const controller = new SettingsController(vi.fn());
    controller.setState(settingsView());

    const save = controller.save(settingsView('light').settings);
    await vi.waitFor(() => expect(mocks.saveAppSettings).toHaveBeenCalledTimes(1));
    controller.acceptExternalState(settingsView('dark', 2, 1));
    finishSave?.(settingsView('light', 1));

    await save;
    await vi.waitFor(() => expect(controller.state?.settingsRevision).toBe(2));
    expect(controller.state?.accountRevision).toBe(1);
    expect(controller.state?.settings.theme).toBe('dark');
    await vi.waitFor(() => expect(mocks.getAppSettings).toHaveBeenCalledTimes(1));
  });

  it('does not replace a mutation response with an equal-revision queued event', async () => {
    let finishSave: ((state: SettingsViewState) => void) | undefined;
    mocks.saveAppSettings.mockImplementation(
      () => new Promise<SettingsViewState>((resolve) => (finishSave = resolve)),
    );
    mocks.getAppSettings.mockRejectedValue(new Error('offline'));
    const controller = new SettingsController(vi.fn());
    controller.setState(settingsView());

    const save = controller.save(settingsView('light').settings);
    await vi.waitFor(() => expect(mocks.saveAppSettings).toHaveBeenCalledTimes(1));
    controller.acceptExternalState(settingsView('dark', 1));
    finishSave?.(settingsView('light', 1));

    await save;
    expect(controller.state?.settings.theme).toBe('light');
    expect(controller.state?.settingsRevision).toBe(1);
  });

  it('never replaces a newer account state with an older response', () => {
    const controller = new SettingsController(vi.fn());
    controller.setState(settingsView('dark', 2, 2));

    controller.setState(settingsView('light', 1, 1));
    controller.acceptExternalState(settingsView('light', 3, 1));

    expect(controller.state?.accountRevision).toBe(2);
    expect(controller.state?.settingsRevision).toBe(2);
    expect(controller.state?.settings.theme).toBe('dark');
  });

  it('preserves a local draft until its queued external state can be reconciled', async () => {
    let finishSave: ((state: SettingsViewState) => void) | undefined;
    mocks.saveAppSettings.mockImplementation(
      () => new Promise<SettingsViewState>((resolve) => (finishSave = resolve)),
    );
    mocks.getAppSettings.mockRejectedValue(new Error('offline'));
    const controller = new SettingsController(vi.fn());
    controller.setState(settingsView());
    controller.beginDraft();
    controller.setDraftSettings(settingsView('light').settings);

    controller.acceptExternalState(settingsView('dark', 1));
    await controller.refreshIfIdle();
    expect(controller.state?.settings.theme).toBe('light');
    expect(mocks.getAppSettings).not.toHaveBeenCalled();

    const save = controller.save(controller.state!.settings);
    controller.endDraft();
    await vi.waitFor(() => expect(mocks.saveAppSettings).toHaveBeenCalledTimes(1));
    finishSave?.(settingsView('light', 1));
    await save;

    expect(controller.state?.settings.theme).toBe('light');
    expect(controller.state?.settingsRevision).toBe(1);
  });

  it('uses the same queue and revisions for non-save mutations', async () => {
    let finishSave: ((state: SettingsViewState) => void) | undefined;
    mocks.saveAppSettings.mockImplementation(
      () => new Promise<SettingsViewState>((resolve) => (finishSave = resolve)),
    );
    const mutation = vi.fn().mockResolvedValue(settingsView('system', 2));
    const controller = new SettingsController(vi.fn());
    controller.setState(settingsView());

    const save = controller.save(settingsView('light').settings);
    const reset = controller.runMutation(mutation);

    await vi.waitFor(() => expect(mocks.saveAppSettings).toHaveBeenCalledTimes(1));
    expect(mutation).not.toHaveBeenCalled();
    finishSave?.(settingsView('light', 1));
    await vi.waitFor(() => expect(mutation).toHaveBeenCalledWith(1, 0));
    await Promise.all([save, reset]);
    expect(controller.state?.settingsRevision).toBe(2);
  });

  it('cancels queued snapshot mutations and reloads after a failure', async () => {
    const onError = vi.fn();
    mocks.saveAppSettings.mockRejectedValue('Settings changed.');
    mocks.getAppSettings.mockResolvedValue(settingsView('dark', 1));
    const mutation = vi.fn().mockResolvedValue(settingsView('system', 2));
    const controller = new SettingsController(onError);
    controller.setState(settingsView());

    const save = controller.save(settingsView('light').settings);
    const reset = controller.runMutation(mutation);

    await expect(reset).rejects.toThrow('Settings changed before this operation could start.');
    await save;
    expect(mutation).not.toHaveBeenCalled();
    expect(mocks.getAppSettings).toHaveBeenCalledTimes(1);
    expect(controller.state?.settings.theme).toBe('dark');
    expect(onError).toHaveBeenCalledWith('Settings could not be saved.');
  });
});
