import type { EasingFunction, TransitionConfig } from 'svelte/transition';

interface PageTransitionOptions {
  direction: number;
  duration: number;
  easing: EasingFunction;
}

type AppScreen = 'dashboard' | 'customize' | 'settings' | `provider:${string}`;

export function shouldSlideBetweenScreens(from: AppScreen, to: AppScreen) {
  const hasStablePanelHeight = (screen: AppScreen) =>
    screen === 'dashboard' || screen === 'settings';
  return hasStablePanelHeight(from) && hasStablePanelHeight(to);
}

export function horizontalPageTransition(
  _node: Element,
  { direction, duration, easing }: PageTransitionOptions,
): TransitionConfig {
  const distance = _node.getBoundingClientRect().width * direction;
  return {
    duration,
    easing,
    css: (t) => `transform: translate3d(${(1 - t) * distance}px, 0, 0);`,
  };
}
