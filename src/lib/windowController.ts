import { currentMonitor, getCurrentWindow } from '@tauri-apps/api/window';
import { resizeMainWindow } from './backend';
import { panelTargetHeight, screenPanelHeight, shouldDeferPanelFit } from './panelSizing';

export type AppScreen = 'dashboard' | 'customize' | 'settings' | `provider:${string}`;

interface WindowControllerOptions {
  screen: () => AppScreen;
  refreshing: () => boolean;
  reordering: () => boolean;
  reducedMotion: () => boolean;
  onError: (message: string) => void;
}

export function createWindowController(options: WindowControllerOptions) {
  let measureFrame = 0;
  let resizeFrame = 0;
  let resizeGeneration = 0;
  let resizeAvailable = true;
  let dashboardHeight: number | null = null;

  function shouldDefer() {
    return options.reordering() || shouldDeferPanelFit(options.screen(), options.refreshing());
  }

  function cancelPending() {
    if (typeof window === 'undefined') return;
    window.cancelAnimationFrame(measureFrame);
    window.cancelAnimationFrame(resizeFrame);
    resizeGeneration += 1;
  }

  function scheduleFit() {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window) || !resizeAvailable)
      return;
    if (shouldDefer()) {
      cancelPending();
      return;
    }
    window.cancelAnimationFrame(measureFrame);
    measureFrame = window.requestAnimationFrame(() => void fit());
  }

  async function fit() {
    if (shouldDefer()) return;
    const screen = options.screen();
    const page = document.querySelector<HTMLElement>(`.screen-page[data-screen="${screen}"]`);
    const content = document.querySelector<HTMLElement>('.content');
    const stage = document.querySelector<HTMLElement>('.screen-stage');
    const header = document.querySelector<HTMLElement>('.screen-header');
    const footer = document.querySelector<HTMLElement>('.footer');
    if (!page || !content || !stage) return;

    const pageHeight = page.scrollHeight;
    stage.style.height = `${pageHeight}px`;
    const contentStyle = window.getComputedStyle(content);
    const contentPadding =
      Number.parseFloat(contentStyle.paddingTop) + Number.parseFloat(contentStyle.paddingBottom);
    const idealHeight =
      pageHeight + contentPadding + (header?.offsetHeight ?? 0) + (footer?.offsetHeight ?? 0);
    const appWindow = getCurrentWindow();
    const monitor = await currentMonitor().catch(() => null);
    const workAreaHeight = monitor
      ? monitor.workArea.size.height / monitor.scaleFactor
      : window.screen.availHeight;
    const scale = await appWindow.scaleFactor();
    const current = (await appWindow.innerSize()).height / scale;
    const contentTarget = panelTargetHeight(idealHeight, workAreaHeight);
    const target = screenPanelHeight(screen, contentTarget, dashboardHeight ?? Math.round(current));
    if (screen === 'dashboard') dashboardHeight = target;

    const generation = ++resizeGeneration;
    window.cancelAnimationFrame(resizeFrame);
    if (options.reducedMotion()) {
      await resize(target);
      return;
    }
    if (Math.abs(current - target) < 1) return;

    const started = performance.now();
    const duration = 420;
    const animate = (time: number) => {
      if (generation !== resizeGeneration) return;
      const progress = Math.min(1, (time - started) / duration);
      const eased = 1 - Math.pow(1 - progress, 3);
      void resize(Math.round(current + (target - current) * eased));
      if (progress < 1) resizeFrame = window.requestAnimationFrame(animate);
    };
    resizeFrame = window.requestAnimationFrame(animate);
  }

  async function resize(height: number) {
    try {
      await resizeMainWindow(height);
    } catch {
      resizeAvailable = false;
      options.onError('OpenQuota window could not adapt to its content.');
    }
  }

  return {
    scheduleFit,
    dispose: cancelPending,
  };
}
