import { currentMonitor, getCurrentWindow } from '@tauri-apps/api/window';
import { resizeMainWindow } from './backend';
import { springMotion } from './motion';
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
  let contentMorphActive = false;
  let contentMorphTimer: ReturnType<typeof setTimeout> | undefined;
  let resizeInFlight = false;
  let pendingResizeHeight: number | null = null;

  function shouldDefer() {
    return options.reordering() || shouldDeferPanelFit(options.screen(), options.refreshing());
  }

  function cancelPending() {
    if (typeof window === 'undefined') return;
    window.cancelAnimationFrame(measureFrame);
    window.cancelAnimationFrame(resizeFrame);
    pendingResizeHeight = null;
    resizeGeneration += 1;
  }

  function beginContentMorph() {
    if (typeof window === 'undefined') return;
    // The DOM transition becomes the single animation clock. While it is active, ResizeObserver
    // measurements are forwarded straight to the native window instead of starting a second spring
    // that would continually chase the content from behind.
    cancelPending();
    window.clearTimeout(contentMorphTimer);
    contentMorphActive = !options.reducedMotion();
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

    // scrollHeight exposes the final overflow size as soon as an intro node mounts, which makes the
    // native window jump to the end before Svelte's slide has rendered its first frame. The border box
    // is the height actually on screen at this point in the transition and therefore the value the
    // native panel must follow. JSDOM/non-layout environments retain the intrinsic fallback.
    const renderedPageHeight = page.getBoundingClientRect().height;
    const pageHeight =
      renderedPageHeight > 0 ? renderedPageHeight : page.offsetHeight || page.scrollHeight;
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
    const contentTarget = panelTargetHeight(idealHeight, workAreaHeight);
    if (screen === 'dashboard' && (options.reducedMotion() || contentMorphActive)) {
      dashboardHeight = contentTarget;
      ++resizeGeneration;
      window.cancelAnimationFrame(resizeFrame);
      await resize(contentTarget);
      return;
    }

    const scale = await appWindow.scaleFactor();
    const current = (await appWindow.innerSize()).height / scale;
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
    const motion = springMotion(false);
    const duration = motion.duration;
    const animate = (time: number) => {
      if (generation !== resizeGeneration) return;
      const progress = Math.min(1, (time - started) / duration);
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
      while (pendingResizeHeight !== null && resizeAvailable) {
        const nextHeight = pendingResizeHeight;
        pendingResizeHeight = null;
        await resizeMainWindow(nextHeight);
      }
    } catch {
      pendingResizeHeight = null;
      resizeAvailable = false;
      options.onError('OpenQuota window could not adapt to its content.');
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
      cancelPending();
    },
  };
}
