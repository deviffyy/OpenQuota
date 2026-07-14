import { cleanup, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  currentMonitor: vi.fn(),
  getCurrentWindow: vi.fn(),
  resizeMainWindow: vi.fn(),
}));

vi.mock('@tauri-apps/api/window', () => ({
  currentMonitor: mocks.currentMonitor,
  getCurrentWindow: mocks.getCurrentWindow,
}));

vi.mock('./backend', () => ({
  resizeMainWindow: mocks.resizeMainWindow,
}));

import { createWindowController } from './windowController';

describe('window controller content morphing', () => {
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
      </main>
    `;
    const page = document.querySelector<HTMLElement>('.screen-page')!;
    const header = document.querySelector<HTMLElement>('.screen-header')!;
    const footer = document.querySelector<HTMLElement>('.footer')!;
    Object.defineProperty(page, 'scrollHeight', { configurable: true, value: 700 });
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

    mocks.currentMonitor.mockResolvedValue({
      scaleFactor: 1,
      workArea: { size: { width: 1280, height: 1000 } },
    });
    mocks.getCurrentWindow.mockReturnValue({
      scaleFactor: vi.fn().mockResolvedValue(1),
      innerSize: vi.fn().mockResolvedValue({ width: 320, height: 200 }),
    });
    mocks.resizeMainWindow.mockResolvedValue(undefined);
  });

  afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.clearAllMocks();
  });

  it('lets the DOM transition drive each native resize frame directly', async () => {
    const controller = createWindowController({
      screen: () => 'dashboard',
      refreshing: () => false,
      reordering: () => false,
      reducedMotion: () => false,
      onError: vi.fn(),
    });

    controller.beginContentMorph();
    controller.scheduleFit();

    await waitFor(() => expect(mocks.resizeMainWindow).toHaveBeenCalled());
    expect(mocks.resizeMainWindow.mock.calls[0]).toEqual([420]);
    expect(mocks.getCurrentWindow.mock.results[0]?.value.innerSize).not.toHaveBeenCalled();
    expect(document.querySelector<HTMLElement>('.screen-stage')).toHaveStyle({ height: '300px' });

    controller.dispose();
  });

  it('coalesces native resize work so stale transition frames cannot replay later', async () => {
    let releaseFirstResize!: () => void;
    mocks.resizeMainWindow.mockImplementationOnce(
      () => new Promise<void>((resolve) => (releaseFirstResize = resolve)),
    );
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
      screen: () => 'dashboard',
      refreshing: () => false,
      reordering: () => false,
      reducedMotion: () => false,
      onError: vi.fn(),
    });

    controller.beginContentMorph();
    controller.scheduleFit();
    await waitFor(() => expect(mocks.resizeMainWindow).toHaveBeenCalledWith(420));

    renderedHeight = 360;
    controller.scheduleFit();
    await waitFor(() =>
      expect(document.querySelector<HTMLElement>('.screen-stage')).toHaveStyle({ height: '360px' }),
    );
    renderedHeight = 430;
    controller.scheduleFit();
    await waitFor(() =>
      expect(document.querySelector<HTMLElement>('.screen-stage')).toHaveStyle({ height: '430px' }),
    );
    expect(mocks.resizeMainWindow).toHaveBeenCalledTimes(1);

    releaseFirstResize();
    await waitFor(() => expect(mocks.resizeMainWindow).toHaveBeenCalledTimes(2));
    expect(mocks.resizeMainWindow.mock.calls[1]).toEqual([550]);

    controller.dispose();
  });

  it('follows a closing frame and keeps oversized content at the scroll cap', async () => {
    const page = document.querySelector<HTMLElement>('.screen-page')!;
    let renderedHeight = 900;
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
      screen: () => 'dashboard',
      refreshing: () => false,
      reordering: () => false,
      reducedMotion: () => false,
      onError: vi.fn(),
    });

    controller.beginContentMorph();
    controller.scheduleFit();
    await waitFor(() => expect(mocks.resizeMainWindow).toHaveBeenLastCalledWith(850));

    renderedHeight = 250;
    controller.scheduleFit();
    await waitFor(() => expect(mocks.resizeMainWindow).toHaveBeenLastCalledWith(370));

    controller.dispose();
  });
});
