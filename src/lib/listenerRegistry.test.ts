import { describe, expect, it, vi } from 'vitest';
import { createListenerRegistry } from './listenerRegistry';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

describe('listener registry', () => {
  it('stops listeners registered before disposal', async () => {
    const stop = vi.fn();
    const registry = createListenerRegistry(vi.fn());
    registry.add(Promise.resolve(stop));
    await Promise.resolve();

    registry.dispose();

    expect(stop).toHaveBeenCalledOnce();
  });

  it('stops a listener that resolves after disposal', async () => {
    const registration = deferred<() => void>();
    const stop = vi.fn();
    const registry = createListenerRegistry(vi.fn());
    registry.add(registration.promise);

    registry.dispose();
    registration.resolve(stop);
    await Promise.resolve();

    expect(stop).toHaveBeenCalledOnce();
  });

  it('reports active registration failures once and ignores failures after disposal', async () => {
    const onError = vi.fn();
    const first = deferred<() => void>();
    const second = deferred<() => void>();
    const registry = createListenerRegistry(onError);
    registry.add(first.promise);
    registry.add(second.promise);

    first.reject(new Error('event bridge unavailable'));
    second.reject(new Error('event bridge unavailable'));
    await vi.waitFor(() => expect(onError).toHaveBeenCalledOnce());

    const late = deferred<() => void>();
    registry.add(late.promise);
    registry.dispose();
    late.reject(new Error('disposed'));
    await Promise.resolve();
    await Promise.resolve();
    expect(onError).toHaveBeenCalledOnce();
  });
});
