import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import type { SettingsViewState, UpdateProgress } from './lib/types';
import { liveState, providerCatalog, settingsState } from './test/appFixtures';

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
      ]).then(([usage, settings]) => ({ usage, settings, catalog: providerCatalog }));
    }
    return implementation(command, args);
  });
}

describe('OpenQuota update lifecycle', () => {
  beforeEach(() => {
    mocks.currentMonitor.mockResolvedValue({
      scaleFactor: 1,
      workArea: { size: { width: 1280, height: 700 } },
    });
    mocks.listen.mockReset().mockResolvedValue(vi.fn());
    mocks.invoke.mockReset();
    mockInvoke((command: string, args?: InvokeArgs) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      if (command === 'save_app_settings')
        return Promise.resolve({
          ...settingsState,
          settings: args?.settings ?? settingsState.settings,
        });
      if (command === 'resize_main_window') return Promise.resolve();
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
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it('checks for updates manually and persists the macOS menu bar style', async () => {
    vi.spyOn(window.navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)',
    );
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Check for Updates…' }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('check_for_updates'));
    expect(await screen.findByText('OpenQuota 0.1.0 is up to date.')).toBeInTheDocument();
    expect(document.querySelector('.settings-update-status')).toBeNull();
    expect(mocks.invoke).toHaveBeenCalledWith(
      'save_app_settings',
      expect.objectContaining({
        settings: expect.objectContaining({ lastUpdateCheckAt: expect.any(String) }),
      }),
    );
    await fireEvent.click(screen.getByRole('combobox', { name: 'Icon Style' }));
    await fireEvent.click(screen.getByRole('option', { name: 'Bars' }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        'save_app_settings',
        expect.objectContaining({ settings: expect.objectContaining({ menuBarStyle: 'bars' }) }),
      ),
    );
  });

  it('hides the macOS-only icon style on other desktop platforms', async () => {
    vi.spyOn(window.navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
    );
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.queryByRole('combobox', { name: 'Icon Style' })).not.toBeInTheDocument();
  });

  it('surfaces an available update on the dashboard and allows it to be dismissed', async () => {
    mockInvoke((command: string, args?: InvokeArgs) => {
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
    });
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
    mockInvoke((command: string) => {
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
    await fireEvent.click(await screen.findByRole('button', { name: 'Download from GitHub' }));
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
    mockInvoke((command: string) => {
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
      payload: { phase: 'retrying', downloaded: 42, total: 100, percent: 42 },
    });
    expect(await screen.findByText('Download interrupted. Retrying…')).toBeInTheDocument();

    progressListener?.({
      payload: { phase: 'installing', downloaded: 100, total: 100, percent: 100 },
    });
    expect(await screen.findByText('Installing update…')).toBeInTheDocument();
  });

  it('explains recoverable update failures and offers safe fallback actions', async () => {
    mockInvoke((command: string) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      if (command === 'save_app_settings') return Promise.resolve(settingsState);
      if (command === 'check_for_updates')
        return Promise.resolve({
          available: true,
          currentVersion: '0.1.0',
          version: '0.2.0',
          body: null,
          installable: true,
          releaseUrl: 'https://github.com/deviffyy/OpenQuota/releases/latest',
        });
      if (command === 'install_update')
        return Promise.reject({
          code: 'download_forbidden',
          message: 'GitHub refused the update download.',
          action: 'Try again or download it from the release page.',
          retryable: true,
        });
      if (command === 'open_update_page') return Promise.resolve();
      return Promise.resolve();
    });

    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Check for Updates…' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Install Update' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'GitHub refused the update download.',
    );
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Try again or download it from the release page.',
    );
    expect(screen.getByRole('button', { name: 'Try Again' })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'View Release' }));
    expect(mocks.invoke).toHaveBeenCalledWith('open_update_page');
  });
});
