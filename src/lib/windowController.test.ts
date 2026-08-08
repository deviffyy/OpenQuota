import { cleanup, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  currentMonitor: vi.fn(),
  getCurrentWindow: vi.fn(),
  fitPanelToContent: vi.fn(),
}));

vi.mock('@tauri-apps/api/window', () => ({
  currentMonitor: mocks.currentMonitor,
  getCurrentWindow: mocks.getCurrentWindow,
}));

vi.mock('./backend', () => ({
  fitPanelToContent: mocks.fitPanelToContent,
}));

import { createWindowController } from './windowController';

describe('hybrid window controller', () => {
  beforeEach(() => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    });
    document.body.innerHTML = `
      <main class="content" style="padding: 10px 0">
        <header class="screen-header"></header>
        <div class="screen-stage">
          <div class="screen-page" data-screen="dashboard"></div>
        </div>
        <footer class="footer"></footer>
        <div class="panel-resize-dragger"></div>
      </main>
    `;
    const page = document.querySelector<HTMLElement>('.screen-page')!;
    const header = document.querySelector<HTMLElement>('.screen-header')!;
    const footer = document.querySelector<HTMLElement>('.footer')!;
    const dragger = document.querySelector<HTMLElement>('.panel-resize-dragger')!;
    page.getBoundingClientRect = vi.fn(
      () =>
        ({
          width: 292,
          height: 300,
          top: 0,
          right: 292,
          bottom: 300,
          left: 0,
          x: 0,
          y: 0,
        }) as DOMRect,
    );
    Object.defineProperty(header, 'offsetHeight', { configurable: true, value: 40 });
    Object.defineProperty(footer, 'offsetHeight', { configurable: true, value: 60 });
    Object.defineProperty(dragger, 'offsetHeight', { configurable: true, value: 10 });

    mocks.currentMonitor.mockResolvedValue({
      scaleFactor: 1,
      workArea: { size: { width: 1280, height: 1000 } },
    });
    mocks.getCurrentWindow.mockReturnValue({
      scaleFactor: vi.fn().mockResolvedValue(1),
      innerSize: vi.fn().mockResolvedValue({ width: 320, height: 200 }),
    });
    mocks.fitPanelToContent.mockResolvedValue(true);
  });

  afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.clearAllMocks();
  });

  it('lets an in-content morph drive automatic native fitting directly', async () => {
    const controller = createWindowController({
      screen: () => 'dashboard',
      refreshing: () => false,
      reordering: () => false,
      automatic: () => true,
      reducedMotion: () => false,
      onError: vi.fn(),
    });

    controller.beginContentMorph();
    controller.scheduleFit();

    await waitFor(() => expect(mocks.fitPanelToContent).toHaveBeenCalledWith(430));
    expect(mocks.getCurrentWindow).not.toHaveBeenCalled();
    expect(document.querySelector<HTMLElement>('.screen-stage')).toHaveStyle({ height: '300px' });
    controller.dispose();
  });

  it('keeps measuring layout without resizing while the user owns the height', async () => {
    const controller = createWindowController({
      screen: () => 'dashboard',
      refreshing: () => false,
      reordering: () => false,
      automatic: () => false,
      reducedMotion: () => false,
      onError: vi.fn(),
    });

    controller.scheduleFit();

    await waitFor(() =>
      expect(document.querySelector<HTMLElement>('.screen-stage')).toHaveStyle({ height: '300px' }),
    );
    expect(mocks.fitPanelToContent).not.toHaveBeenCalled();
    controller.dispose();
  });

  it('uses the monitor cap for oversized automatic content', async () => {
    const page = document.querySelector<HTMLElement>('.screen-page')!;
    page.getBoundingClientRect = vi.fn(
      () =>
        ({
          width: 292,
          height: 900,
          top: 0,
          right: 292,
          bottom: 900,
          left: 0,
          x: 0,
          y: 0,
        }) as DOMRect,
    );
    const controller = createWindowController({
      screen: () => 'dashboard',
      refreshing: () => false,
      reordering: () => false,
      automatic: () => true,
      reducedMotion: () => true,
      onError: vi.fn(),
    });

    controller.scheduleFit();

    await waitFor(() => expect(mocks.fitPanelToContent).toHaveBeenCalledWith(850));
    controller.dispose();
  });

  it('keeps Settings at the most recently measured dashboard height', async () => {
    let activeScreen: 'dashboard' | 'settings' = 'dashboard';
    const page = document.querySelector<HTMLElement>('.screen-page')!;
    let renderedHeight = 300;
    page.getBoundingClientRect = vi.fn(
      () =>
        ({
          width: 292,
          height: renderedHeight,
          top: 0,
          right: 292,
          bottom: renderedHeight,
          left: 0,
          x: 0,
          y: 0,
        }) as DOMRect,
    );
    const controller = createWindowController({
      screen: () => activeScreen,
      refreshing: () => false,
      reordering: () => false,
      automatic: () => true,
      reducedMotion: () => true,
      onError: vi.fn(),
    });

    controller.scheduleFit();
    await waitFor(() => expect(mocks.fitPanelToContent).toHaveBeenLastCalledWith(430));

    activeScreen = 'settings';
    page.dataset.screen = 'settings';
    renderedHeight = 700;
    controller.scheduleFit();

    await waitFor(() => expect(mocks.fitPanelToContent).toHaveBeenCalledTimes(2));
    expect(mocks.fitPanelToContent).toHaveBeenLastCalledWith(430);
    controller.dispose();
  });

  it('preserves dashboard content height when floating chrome changes in Settings', async () => {
    let activeScreen: 'dashboard' | 'settings' = 'dashboard';
    const page = document.querySelector<HTMLElement>('.screen-page')!;
    let renderedHeight = 300;
    page.getBoundingClientRect = vi.fn(
      () =>
        ({
          width: 292,
          height: renderedHeight,
          top: 0,
          right: 292,
          bottom: renderedHeight,
          left: 0,
          x: 0,
          y: 0,
        }) as DOMRect,
    );
    const chrome = document.createElement('header');
    chrome.className = 'floating-chrome';
    Object.defineProperty(chrome, 'offsetHeight', { configurable: true, value: 32 });
    document.querySelector('main')!.prepend(chrome);
    const controller = createWindowController({
      screen: () => activeScreen,
      refreshing: () => false,
      reordering: () => false,
      automatic: () => true,
      reducedMotion: () => true,
      onError: vi.fn(),
    });

    controller.scheduleFit();
    await waitFor(() => expect(mocks.fitPanelToContent).toHaveBeenLastCalledWith(462));

    activeScreen = 'settings';
    page.dataset.screen = 'settings';
    renderedHeight = 700;
    controller.scheduleFit();
    await waitFor(() => expect(mocks.fitPanelToContent).toHaveBeenCalledTimes(2));
    expect(mocks.fitPanelToContent).toHaveBeenLastCalledWith(462);

    chrome.remove();
    controller.scheduleFit();
    await waitFor(() => expect(mocks.fitPanelToContent).toHaveBeenCalledTimes(3));
    expect(mocks.fitPanelToContent).toHaveBeenLastCalledWith(430);
    controller.dispose();
  });

  it('defers automatic fitting while refresh or reordering can move rows', async () => {
    let refreshing = true;
    const controller = createWindowController({
      screen: () => 'dashboard',
      refreshing: () => refreshing,
      reordering: () => false,
      automatic: () => true,
      reducedMotion: () => true,
      onError: vi.fn(),
    });

    controller.scheduleFit();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(mocks.fitPanelToContent).not.toHaveBeenCalled();

    refreshing = false;
    controller.scheduleFit();
    await waitFor(() => expect(mocks.fitPanelToContent).toHaveBeenCalledWith(430));
    controller.dispose();
  });
});
