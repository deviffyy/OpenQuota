import { describe, expect, it } from 'vitest';
import { horizontalPageTransition, shouldSlideBetweenScreens } from './pageTransition';

describe('page transition', () => {
  const linear = (value: number) => value;

  function page() {
    const element = document.createElement('div');
    element.getBoundingClientRect = () =>
      ({
        width: 292,
        height: 400,
        x: 0,
        y: 0,
        top: 0,
        right: 292,
        bottom: 400,
        left: 0,
      }) as DOMRect;
    return element;
  }

  it('moves pages horizontally without fading the content', () => {
    const transition = horizontalPageTransition(page(), {
      direction: 1,
      duration: 420,
      easing: linear,
    });
    expect(transition.css?.(0, 1)).toContain('translate3d(292px, 0, 0)');
    expect(transition.css?.(1, 0)).toContain('translate3d(0px, 0, 0)');
    expect(transition.css?.(0.5, 0.5)).not.toContain('opacity');
  });

  it('moves the outgoing page in the opposite direction', () => {
    const transition = horizontalPageTransition(page(), {
      direction: -1,
      duration: 420,
      easing: linear,
    });
    expect(transition.css?.(0, 1)).toContain('translate3d(-292px, 0, 0)');
  });

  it('slides only between screens that share a stable panel height', () => {
    expect(shouldSlideBetweenScreens('dashboard', 'settings')).toBe(true);
    expect(shouldSlideBetweenScreens('settings', 'dashboard')).toBe(true);
    expect(shouldSlideBetweenScreens('dashboard', 'customize')).toBe(false);
    expect(shouldSlideBetweenScreens('customize', 'provider:codex')).toBe(false);
    expect(shouldSlideBetweenScreens('provider:codex', 'dashboard')).toBe(false);
  });
});
