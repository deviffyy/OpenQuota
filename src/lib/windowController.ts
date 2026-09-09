import { currentMonitor, getCurrentWindow } from '@tauri-apps/api/window';
import { fitPanelToContent } from './backend';
import { t } from './i18n';
import { springMotion } from './motion';
import { panelTargetHeight, screenPanelHeight, shouldDeferPanelFit } from './panelSizing';

export type AppScreen = 'dashboard' | 'customize' | 'settings' | `provider:${string}`;

interface WindowControllerOptions {
  screen: () => AppScreen;
  refreshing: () => boolean;
  reordering: () => boolean;
  automatic: () => boolean;
  reducedMotion: () => boolean;
  onError: (message: string) => void;
}

function cssPixels(value: string) {
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function createWindowController(options: WindowControllerOptions) {
  let measureFrame = 0;
  let resizeFrame = 0;
  let resizeGeneration = 0;
  let resizeAvailable = true;
  let contentMorphActive = false;
  let contentMorphTimer: ReturnType<typeof setTimeout> | undefined;
  let resizeInFlight = false;
  let pendingResizeHeight: number | null = null;
  let dashboardBodyHeight: number | null = null;

  function shouldDefer() {
    return options.reordering() || shouldDeferPanelFit(options.screen(), options.refreshing());
  }

  function cancelPendingResize() {
    if (typeof window === 'undefined') return;
    window.cancelAnimationFrame(resizeFrame);
    pendingResizeHeight = null;
    resizeGeneration += 1;
  }

  function beginContentMorph() {
    if (typeof window === 'undefined') return;
    cancelPendingResize();
    window.clearTimeout(contentMorphTimer);
    contentMorphActive = options.automatic() && !options.reducedMotion();
    if (!contentMorphActive) {
      scheduleFit();
      return;
    }
    const duration = springMotion(false).duration;
    contentMorphTimer = window.setTimeout(() => {
      contentMorphActive = false;
      scheduleFit();
    }, duration + 34);
  }

  function scheduleFit() {
    if (typeof window === 'undefined') return;
    if (shouldDefer()) {
      window.cancelAnimationFrame(measureFrame);
      cancelPendingResize();
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
    const dragger = document.querySelector<HTMLElement>('.panel-resize-dragger');
    const floatingChrome = document.querySelector<HTMLElement>('.floating-chrome');
    if (!page || !content || !stage) return;

    const renderedHeight = page.getBoundingClientRect().height;
    const pageHeight = renderedHeight > 0 ? renderedHeight : page.offsetHeight || page.scrollHeight;
    stage.style.height = `${pageHeight}px`;

    if (!options.automatic() || !('__TAURI_INTERNALS__' in window) || !resizeAvailable) return;

    const contentStyle = window.getComputedStyle(content);
    const contentPadding =
      cssPixels(contentStyle.paddingTop) + cssPixels(contentStyle.paddingBottom);
    const chromeHeight = floatingChrome?.offsetHeight ?? 0;
    const idealHeight =
      pageHeight +
      contentPadding +
      (header?.offsetHeight ?? 0) +
      (footer?.offsetHeight ?? 0) +
      (dragger?.offsetHeight ?? 0) +
      chromeHeight;
    const monitor = await currentMonitor().catch(() => null);
    const workAreaHeight = monitor
      ? monitor.workArea.size.height / monitor.scaleFactor
      : window.screen.availHeight;
    const contentTarget = panelTargetHeight(idealHeight, workAreaHeight);
    if (screen === 'dashboard') dashboardBodyHeight = contentTarget - chromeHeight;

    let current: number | null = null;
    if (screen === 'settings' && dashboardBodyHeight === null) {
      const appWindow = getCurrentWindow();
      const scale = await appWindow.scaleFactor();
      current = (await appWindow.innerSize()).height / scale;
    }
    const dashboardTarget = panelTargetHeight(
      (dashboardBodyHeight ?? Math.round(current ?? contentTarget) - chromeHeight) + chromeHeight,
      workAreaHeight,
    );
    const target = screenPanelHeight(screen, contentTarget, dashboardTarget);

    if (options.reducedMotion() || contentMorphActive) {
      ++resizeGeneration;
      window.cancelAnimationFrame(resizeFrame);
      await resize(target);
      return;
    }

    if (current === null) {
      const appWindow = getCurrentWindow();
      const scale = await appWindow.scaleFactor();
      current = (await appWindow.innerSize()).height / scale;
    }
    const generation = ++resizeGeneration;
    window.cancelAnimationFrame(resizeFrame);
    if (Math.abs(current - target) < 1) return;

    const started = performance.now();
    const motion = springMotion(false);
    const animate = (time: number) => {
      if (generation !== resizeGeneration || !options.automatic()) return;
      const progress = Math.min(1, (time - started) / motion.duration);
      const eased = motion.easing(progress);
      void resize(Math.round(current + (target - current) * eased));
      if (progress < 1) resizeFrame = window.requestAnimationFrame(animate);
    };
    resizeFrame = window.requestAnimationFrame(animate);
  }

  async function resize(height: number) {
    pendingResizeHeight = height;
    if (resizeInFlight) return;
    resizeInFlight = true;
    try {
      while (pendingResizeHeight !== null && resizeAvailable && options.automatic()) {
        const nextHeight = pendingResizeHeight;
        pendingResizeHeight = null;
        await fitPanelToContent(nextHeight);
      }
    } catch {
      pendingResizeHeight = null;
      resizeAvailable = false;
      options.onError(t('windowAdaptFailed'));
    } finally {
      resizeInFlight = false;
    }
  }

  return {
    beginContentMorph,
    scheduleFit,
    dispose() {
      window.clearTimeout(contentMorphTimer);
      contentMorphActive = false;
      window.cancelAnimationFrame(measureFrame);
      cancelPendingResize();
    },
  };
}
