import { afterEach, describe, expect, it, vi } from 'vitest';
import { beginDrag } from './dragPreview';

afterEach(() => vi.useRealTimers());

describe('drag lift preview', () => {
  it('uses a labeled custom drag image and removes its helper', () => {
    vi.useFakeTimers();
    const setDragImage = vi.fn();
    const transfer = { effectAllowed: 'none', setDragImage } as unknown as DataTransfer;
    beginDrag({ dataTransfer: transfer } as DragEvent, 'Codex', '6 metrics');
    const preview = document.querySelector('.drag-lift-preview');
    expect(preview).toHaveTextContent('Codex6 metrics');
    expect(setDragImage).toHaveBeenCalledWith(preview, 22, 0);
    vi.runAllTimers();
    expect(document.querySelector('.drag-lift-preview')).toBeNull();
  });
});
