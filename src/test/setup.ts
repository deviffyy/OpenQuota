import '@testing-library/jest-dom/vitest';

if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
      dispatchEvent: () => true,
    }) as MediaQueryList;
}

if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

if (!Element.prototype.animate) {
  Element.prototype.animate = () => {
    const animation = {
      cancel() {},
      finish() {},
      play() {},
      pause() {},
      reverse() {},
      addEventListener() {},
      removeEventListener() {},
      dispatchEvent: () => true,
      commitStyles() {},
      persist() {},
      updatePlaybackRate() {},
      currentTime: 0,
      effect: null,
      finished: Promise.resolve(),
      id: '',
      oncancel: null,
      onremove: null,
      pending: false,
      playbackRate: 1,
      playState: 'finished',
      ready: Promise.resolve(),
      replaceState: 'active',
      startTime: 0,
      timeline: null,
    } as unknown as Animation;
    let onfinish: Animation['onfinish'] = null;
    Object.defineProperty(animation, 'onfinish', {
      get: () => onfinish,
      set: (value: Animation['onfinish']) => {
        onfinish = value;
        if (value) {
          queueMicrotask(() =>
            value.call(animation, new Event('finish') as AnimationPlaybackEvent),
          );
        }
      },
    });
    return animation;
  };
}
