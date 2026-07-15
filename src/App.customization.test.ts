import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import type { SettingsViewState } from './lib/types';
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

describe('OpenQuota customization persistence and reorder', () => {
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
      if (command === 'reset_customization') return Promise.resolve(settingsState);
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
  afterEach(cleanup);

  it('uses an in-app confirmation sheet before resetting all customization', async () => {
    const browserConfirm = vi.spyOn(window, 'confirm');
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Customize' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Reset all customization' }));

    const dialog = screen.getByRole('alertdialog', { name: 'Reset All Customization?' });
    expect(dialog).toHaveTextContent('restores every provider');
    expect(browserConfirm).not.toHaveBeenCalled();
    expect(mocks.invoke).not.toHaveBeenCalledWith('reset_customization');

    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: 'Reset all customization' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Reset All' }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('reset_customization'));
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
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

  it('reloads persisted settings when a settings save fails', async () => {
    mockInvoke((command: string, args?: { settings?: SettingsViewState['settings'] }) => {
      if (command === 'get_usage_state') return Promise.resolve(liveState);
      if (command === 'get_app_settings') return Promise.resolve(settingsState);
      if (command === 'save_app_settings') return Promise.reject('Launch at login is unavailable.');
      if (command === 'resize_main_window') return Promise.resolve();
      return Promise.reject(new Error(`unexpected command ${command} ${String(args)}`));
    });
    render(App);
    await screen.findByText('Plus');
    await fireEvent.click(screen.getByLabelText('Open options'));
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    const launchAtLogin = screen.getByRole('checkbox', { name: 'Launch at Login' });

    await fireEvent.click(launchAtLogin);

    await waitFor(() => expect(launchAtLogin).not.toBeChecked());
    expect(screen.getByRole('alert')).toHaveTextContent('Launch at login is unavailable.');
    expect(mocks.invoke).toHaveBeenCalledWith('get_app_settings');
  });

  it('reorders dashboard metrics directly with a custom drag lift', async () => {
    render(App);
    await screen.findByText('Plus');
    const session = screen.getByRole('group', { name: 'Session options' });
    const weekly = screen.getByRole('group', { name: 'Weekly options' });
    const trend = screen.getByRole('group', { name: 'Usage Trend options' });
    session.getBoundingClientRect = () =>
      ({ top: 0, right: 280, bottom: 40, left: 0, width: 280, height: 40 }) as DOMRect;
    weekly.getBoundingClientRect = () =>
      ({ top: 40, right: 280, bottom: 80, left: 0, width: 280, height: 40 }) as DOMRect;
    trend.getBoundingClientRect = () =>
      ({ top: 80, right: 280, bottom: 120, left: 0, width: 280, height: 40 }) as DOMRect;
    const savesBeforeDrag = mocks.invoke.mock.calls.filter(
      ([command]) => command === 'save_app_settings',
    ).length;
    await fireEvent.pointerDown(session, {
      pointerId: 1,
      pointerType: 'mouse',
      button: 0,
      clientX: 20,
      clientY: 20,
    });
    await fireEvent.pointerMove(window, {
      pointerId: 1,
      pointerType: 'mouse',
      clientX: 20,
      clientY: 52,
    });
    expect(document.querySelector('.pointer-reorder-lift')).not.toBeNull();
    await fireEvent.pointerMove(window, {
      pointerId: 1,
      pointerType: 'mouse',
      clientX: 20,
      clientY: 92,
    });
    await fireEvent.pointerUp(window, {
      pointerId: 1,
      pointerType: 'mouse',
      clientX: 20,
      clientY: 52,
    });
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
      'codex.spark',
      'codex.sparkWeekly',
      'codex.trend',
      'codex.session',
      'codex.credits',
      'codex.rateLimitResets',
      'codex.today',
      'codex.yesterday',
      'codex.last30',
    ]);

    const savesBeforeUndo = mocks.invoke.mock.calls.filter(
      ([command]) => command === 'save_app_settings',
    ).length;
    expect(savesBeforeUndo).toBe(savesBeforeDrag + 1);
    await fireEvent.keyDown(document, { key: 'z', ctrlKey: true });
    await waitFor(() =>
      expect(
        mocks.invoke.mock.calls.filter(([command]) => command === 'save_app_settings').length,
      ).toBeGreaterThan(savesBeforeUndo),
    );
    const undoSave = [...mocks.invoke.mock.calls]
      .reverse()
      .find(([command]) => command === 'save_app_settings');
    const restored = undoSave?.[1] as { settings: SettingsViewState['settings'] };
    expect(
      restored.settings.providers
        .find((provider) => provider.id === 'codex')
        ?.metrics.map((metric) => metric.id),
    ).toEqual([
      'codex.session',
      'codex.weekly',
      'codex.spark',
      'codex.sparkWeekly',
      'codex.trend',
      'codex.credits',
      'codex.rateLimitResets',
      'codex.today',
      'codex.yesterday',
      'codex.last30',
    ]);
  });

  it('describes reorder controls for keyboard and assistive-technology users', async () => {
    render(App);
    await screen.findByText('Plus');

    const instructions = screen.getByText(
      'Drag to reorder. With a keyboard, use Alt plus Up Arrow or Alt plus Down Arrow.',
    );
    expect(instructions).toHaveClass('sr-only');
    expect(instructions).toHaveAttribute('id', 'reorder-instructions');
    const handles = document.querySelectorAll<HTMLElement>('[data-reorder-touch-handle]');
    expect(handles.length).toBeGreaterThan(0);
    handles.forEach((handle) => {
      expect(handle).toHaveAttribute('aria-describedby', 'reorder-instructions');
      expect(handle).toHaveAttribute('aria-keyshortcuts', 'Alt+ArrowUp Alt+ArrowDown');
    });
  });

  it('does not let the global Enter shortcut steal an interactive control keypress', async () => {
    render(App);
    await screen.findByText('Plus');
    const handle = screen.getByRole('button', { name: 'Move Session' });

    handle.focus();
    await fireEvent.keyDown(handle, { key: 'Enter' });
    expect(screen.queryByRole('heading', { name: 'Customize' })).not.toBeInTheDocument();
    expect(screen.getByText('Plus')).toBeInTheDocument();

    handle.blur();
    await fireEvent.keyDown(document, { key: 'Enter' });
    expect(await screen.findByRole('heading', { name: 'Customize' })).toBeInTheDocument();
  });

  it('restores the pre-drag layout when a reorder is cancelled', async () => {
    render(App);
    await screen.findByText('Plus');
    const session = screen.getByRole('group', { name: 'Session options' });
    const weekly = screen.getByRole('group', { name: 'Weekly options' });
    session.getBoundingClientRect = () =>
      ({ top: 0, right: 280, bottom: 40, left: 0, width: 280, height: 40 }) as DOMRect;
    weekly.getBoundingClientRect = () =>
      ({ top: 40, right: 280, bottom: 80, left: 0, width: 280, height: 40 }) as DOMRect;
    const savesBeforeDrag = mocks.invoke.mock.calls.filter(
      ([command]) => command === 'save_app_settings',
    ).length;

    await fireEvent.pointerDown(session, {
      pointerId: 1,
      pointerType: 'mouse',
      button: 0,
      clientX: 20,
      clientY: 20,
    });
    await fireEvent.pointerMove(window, {
      pointerId: 1,
      pointerType: 'mouse',
      clientX: 20,
      clientY: 52,
    });
    await fireEvent.keyDown(window, { key: 'Escape' });

    await waitFor(() => {
      const metricIds = [...document.querySelectorAll<HTMLElement>('[data-reorder-id]')]
        .filter((element) => element.dataset.reorderGroup === 'dashboard-metrics:codex')
        .map((element) => element.dataset.reorderId)
        .filter((id) => id !== 'section:onDemand');
      expect(metricIds.slice(0, 3)).toEqual(['codex.session', 'codex.weekly', 'codex.trend']);
    });
    expect(
      mocks.invoke.mock.calls.filter(([command]) => command === 'save_app_settings'),
    ).toHaveLength(savesBeforeDrag);
    expect(document.querySelector('.pointer-reorder-lift')).toBeNull();
  });
});
