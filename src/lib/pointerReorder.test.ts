import { fireEvent, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { pointerReorder, reorderTargetAt, type ReorderFrame } from './pointerReorder';

afterEach(() => {
  document.body.innerHTML = '';
  document.documentElement.classList.remove('is-reordering');
  Reflect.deleteProperty(navigator, 'vibrate');
});

const frames: ReorderFrame[] = [
  { id: 'first', top: 0, right: 200, bottom: 40, left: 0 },
  { id: 'second', top: 40, right: 200, bottom: 80, left: 0 },
  { id: 'third', top: 80, right: 200, bottom: 120, left: 0 },
];

describe('pointer reorder geometry', () => {
  it('waits until the pointer crosses into a lower row', () => {
    expect(reorderTargetAt({ x: 20, y: 44 }, frames, 'first')).toBeNull();
    expect(reorderTargetAt({ x: 20, y: 48 }, frames, 'first')).toBe('second');
  });

  it('uses the matching threshold while moving upward', () => {
    expect(reorderTargetAt({ x: 20, y: 76 }, frames, 'third')).toBeNull();
    expect(reorderTargetAt({ x: 20, y: 72 }, frames, 'third')).toBe('second');
  });

  it('ignores points outside the reorder column', () => {
    expect(reorderTargetAt({ x: 240, y: 60 }, frames, 'first')).toBeNull();
  });
});

function rect(top: number, bottom = top + 40): DOMRect {
  return { top, right: 200, bottom, left: 0, width: 200, height: bottom - top } as DOMRect;
}

function reorderFixture() {
  const source = document.createElement('div');
  source.dataset.reorderGroup = 'test';
  source.dataset.reorderId = 'first';
  source.getBoundingClientRect = () => rect(0);
  const header = document.createElement('span');
  header.dataset.reorderHandle = '';
  const grip = document.createElement('span');
  grip.dataset.reorderHandle = '';
  grip.dataset.reorderTouchHandle = '';
  header.append(grip);
  source.append(header);

  const target = document.createElement('div');
  target.dataset.reorderGroup = 'test';
  target.dataset.reorderId = 'second';
  target.getBoundingClientRect = () => rect(40);
  document.body.append(source, target);
  return { source, header, grip, target };
}

describe('pointer reorder interaction', () => {
  it('requires the explicit grip on touch while keeping the wider desktop handle', async () => {
    const { source, header, grip } = reorderFixture();
    const onReorder = vi.fn();
    const vibrate = vi.fn();
    Object.defineProperty(navigator, 'vibrate', { configurable: true, value: vibrate });
    const action = pointerReorder(source, {
      id: 'first',
      group: 'test',
      touchGripOnly: true,
      onReorder,
    });

    await fireEvent.pointerDown(header, {
      pointerId: 1,
      pointerType: 'touch',
      clientX: 20,
      clientY: 20,
    });
    await fireEvent.pointerMove(window, {
      pointerId: 1,
      pointerType: 'touch',
      clientX: 20,
      clientY: 52,
    });
    await fireEvent.pointerUp(window, { pointerId: 1, pointerType: 'touch' });
    expect(onReorder).not.toHaveBeenCalled();

    await fireEvent.pointerDown(grip, {
      pointerId: 2,
      pointerType: 'touch',
      clientX: 20,
      clientY: 20,
    });
    await fireEvent.pointerMove(window, {
      pointerId: 2,
      pointerType: 'touch',
      clientX: 20,
      clientY: 52,
    });
    await fireEvent.pointerMove(window, {
      pointerId: 2,
      pointerType: 'touch',
      clientX: 20,
      clientY: 20,
    });
    await fireEvent.pointerMove(window, {
      pointerId: 2,
      pointerType: 'touch',
      clientX: 20,
      clientY: 52,
    });
    await fireEvent.pointerUp(window, { pointerId: 2, pointerType: 'touch' });
    expect(onReorder).toHaveBeenCalledWith('second');
    expect(onReorder).toHaveBeenCalledTimes(2);
    expect(vibrate).toHaveBeenCalledWith(8);
    expect(vibrate).toHaveBeenCalledTimes(1);
    action.destroy();
  });

  it('cancels an active gesture with Escape', async () => {
    const { source, grip } = reorderFixture();
    const onEnd = vi.fn();
    const action = pointerReorder(source, {
      id: 'first',
      group: 'test',
      label: 'Session',
      onReorder: vi.fn(),
      onEnd,
    });
    grip.tabIndex = 0;
    grip.focus();

    await fireEvent.pointerDown(grip, {
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

    expect(onEnd).toHaveBeenCalledWith(true, true);
    expect(document.querySelector('.pointer-reorder-lift')).toBeNull();
    expect(document.documentElement).not.toHaveClass('is-reordering');
    await waitFor(() => {
      expect(document.activeElement).toBe(grip);
      expect(document.querySelector('[data-reorder-announcer]')).toHaveTextContent(
        'Session move cancelled.',
      );
    });
    action.destroy();
  });

  it('announces the final keyboard position and follows a replaced source row', async () => {
    const { source, grip, target } = reorderFixture();
    grip.tabIndex = 0;
    let replacementGrip: HTMLElement | null = null;
    const action = pointerReorder(source, {
      id: 'first',
      group: 'test',
      label: 'Session',
      gripOnly: true,
      onReorder: () => {
        const replacement = source.cloneNode(true) as HTMLElement;
        replacementGrip = replacement.querySelector<HTMLElement>(
          '[data-reorder-handle][tabindex="0"]',
        );
        target.after(replacement);
        source.remove();
      },
    });

    grip.focus();
    await fireEvent.keyDown(grip, { key: 'ArrowDown', altKey: true });

    await waitFor(() =>
      expect(document.querySelector('[data-reorder-announcer]')).toHaveTextContent(
        'Session moved to position 2 of 2.',
      ),
    );
    expect(document.activeElement).toBe(replacementGrip);
    action.destroy();
  });

  it('keeps the lifted preview inside the visible window', async () => {
    const { source, header } = reorderFixture();
    const previewButton = document.createElement('button');
    previewButton.autofocus = true;
    source.append(previewButton);
    const action = pointerReorder(source, {
      id: 'first',
      group: 'test',
      onReorder: vi.fn(),
    });

    await fireEvent.pointerDown(header, {
      pointerId: 1,
      pointerType: 'mouse',
      button: 0,
      clientX: 20,
      clientY: 20,
    });
    await fireEvent.pointerMove(window, {
      pointerId: 1,
      pointerType: 'mouse',
      clientX: window.innerWidth + 500,
      clientY: window.innerHeight + 500,
    });
    const lift = document.querySelector<HTMLElement>('.pointer-reorder-lift')!;
    expect(lift.parentElement).toHaveClass('pointer-reorder-layer');
    expect(lift).not.toHaveAttribute('inert');
    expect(lift.querySelector('button')).toHaveAttribute('tabindex', '-1');
    expect(lift.querySelector('button')).not.toHaveAttribute('autofocus');
    expect(Number.parseFloat(lift.style.left)).toBeLessThanOrEqual(window.innerWidth - 200);
    expect(Number.parseFloat(lift.style.top)).toBeLessThanOrEqual(window.innerHeight - 40);

    await fireEvent.pointerMove(window, {
      pointerId: 1,
      pointerType: 'mouse',
      clientX: -500,
      clientY: -500,
    });
    expect(Number.parseFloat(lift.style.left)).toBeGreaterThanOrEqual(0);
    expect(Number.parseFloat(lift.style.top)).toBeGreaterThanOrEqual(0);
    await fireEvent.pointerUp(window, { pointerId: 1, pointerType: 'mouse' });
    expect(document.querySelector('.pointer-reorder-layer')).toBeNull();
    action.destroy();
  });

  it('auto-scrolls the nearest overflowing panel near its edge', async () => {
    const scroller = document.createElement('div');
    scroller.style.overflowY = 'auto';
    Object.defineProperties(scroller, {
      clientHeight: { value: 100 },
      scrollHeight: { value: 320 },
    });
    scroller.getBoundingClientRect = () => rect(0, 100);
    const { source, header, target } = reorderFixture();
    scroller.append(source, target);
    document.body.append(scroller);
    const action = pointerReorder(source, {
      id: 'first',
      group: 'test',
      onReorder: vi.fn(),
    });

    await fireEvent.pointerDown(header, {
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
      clientY: 96,
    });
    await waitFor(() => expect(scroller.scrollTop).toBeGreaterThan(0));
    await fireEvent.pointerUp(window, { pointerId: 1, pointerType: 'mouse' });
    action.destroy();
  });
});
