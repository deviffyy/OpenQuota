export interface ReorderPoint {
  x: number;
  y: number;
}

export interface ReorderFrame {
  id: string;
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface PointerReorderOptions {
  id: string;
  group: string;
  label?: string;
  disabled?: boolean;
  gripOnly?: boolean;
  touchGripOnly?: boolean;
  preview?: () => HTMLElement | null;
  onReorder: (targetId: string) => void;
  onStart?: () => void;
  onEnd?: (moved: boolean, cancelled?: boolean) => void;
}

const START_DISTANCE = 4;
const CROSSING_THRESHOLD = 0.2;
const HAPTIC_INTERVAL = 120;
let activeCleanup: (() => void) | null = null;
let lastHapticAt = Number.NEGATIVE_INFINITY;

function hapticSnap(pointerType: string) {
  if (pointerType !== 'touch' || typeof navigator.vibrate !== 'function') return;
  const now = performance.now();
  if (now - lastHapticAt < HAPTIC_INTERVAL) return;
  lastHapticAt = now;
  try {
    navigator.vibrate(8);
  } catch {
    // Unsupported or policy-blocked haptics remain a silent progressive enhancement.
  }
}

function reorderAnnouncementRegion() {
  const existing = document.querySelector<HTMLElement>('[data-reorder-announcer]');
  if (existing) return existing;
  const region = document.createElement('div');
  region.className = 'sr-only';
  region.dataset.reorderAnnouncer = '';
  region.setAttribute('role', 'status');
  region.setAttribute('aria-live', 'polite');
  region.setAttribute('aria-atomic', 'true');
  document.body.append(region);
  return region;
}

function announce(message: string) {
  const region = reorderAnnouncementRegion();
  region.textContent = '';
  window.setTimeout(() => (region.textContent = message), 0);
}

function announceMove(options: PointerReorderOptions, targetId?: string | null) {
  const label = options.label ?? 'Item';
  queueMicrotask(() => {
    const entries = reorderElements(options.group).filter(
      (entry) => !entry.id.startsWith('section:'),
    );
    const position = entries.findIndex((entry) => entry.id === options.id);
    const section = targetId?.startsWith('section:')
      ? targetId
          .slice('section:'.length)
          .replace(/([A-Z])/g, ' $1')
          .toLowerCase()
      : null;
    const message =
      position >= 0
        ? `${label} moved to position ${position + 1} of ${entries.length}.`
        : section
          ? `${label} moved to ${section}.`
          : `${label} moved.`;
    announce(message);
  });
}

function focusTargetFor(element: HTMLElement | undefined | null) {
  if (!element) return null;
  const handle = element.querySelector<HTMLElement>(
    'button[data-reorder-handle], [data-reorder-handle][tabindex="0"]',
  );
  if (handle) return handle;
  return element.matches('button, a, input, select, textarea, [tabindex="0"]') ? element : null;
}

export function reorderTargetAt(point: ReorderPoint, frames: ReorderFrame[], draggedId: string) {
  const from = frames.findIndex((frame) => frame.id === draggedId);
  if (from < 0) return null;

  for (let to = 0; to < frames.length; to += 1) {
    const frame = frames[to];
    if (frame.id === draggedId) continue;
    if (
      point.x < frame.left ||
      point.x > frame.right ||
      point.y < frame.top - 2 ||
      point.y > frame.bottom + 2
    )
      continue;

    const height = Math.max(1, frame.bottom - frame.top);
    if (to > from && point.y < frame.top + height * CROSSING_THRESHOLD) return null;
    if (to < from && point.y > frame.bottom - height * CROSSING_THRESHOLD) return null;
    return frame.id;
  }
  return null;
}

function interactiveTarget(target: EventTarget | null, root: HTMLElement) {
  if (!(target instanceof Element)) return false;
  const interactive = target.closest(
    'button, input, label, a, select, textarea, [contenteditable], [role="button"]',
  );
  return interactive !== null && root.contains(interactive);
}

function scrollParent(node: HTMLElement) {
  let parent = node.parentElement;
  while (parent) {
    const { overflowY } = window.getComputedStyle(parent);
    if (/(auto|scroll)/.test(overflowY) && parent.scrollHeight > parent.clientHeight) return parent;
    parent = parent.parentElement;
  }
  return null;
}

function edgeScrollDelta(container: HTMLElement, point: ReorderPoint) {
  const rect = container.getBoundingClientRect();
  const edge = Math.min(52, rect.height * 0.22);
  if (edge <= 0 || point.y < rect.top || point.y > rect.bottom) return 0;
  if (point.y < rect.top + edge) return -Math.ceil(((rect.top + edge - point.y) / edge) * 12);
  if (point.y > rect.bottom - edge)
    return Math.ceil(((point.y - (rect.bottom - edge)) / edge) * 12);
  return 0;
}

function reorderElements(group: string) {
  return [...document.querySelectorAll<HTMLElement>('[data-reorder-group][data-reorder-id]')]
    .filter(
      (element) =>
        element.dataset.reorderGroup === group && !element.closest('.pointer-reorder-lift'),
    )
    .map((element) => ({
      element,
      id: element.dataset.reorderId ?? '',
      rect: element.getBoundingClientRect(),
    }))
    .filter((entry) => entry.id.length > 0);
}

function makeLift(source: HTMLElement, start: ReorderPoint) {
  const rect = source.getBoundingClientRect();
  const lift = source.cloneNode(true) as HTMLElement;
  lift.classList.remove('reorder-source');
  lift.classList.add('pointer-reorder-lift');
  lift.setAttribute('aria-hidden', 'true');
  lift.inert = true;
  lift.style.width = `${rect.width}px`;
  lift.style.height = `${rect.height}px`;
  document.body.append(lift);

  const offsetX = Math.min(Math.max(0, start.x - rect.left), rect.width);
  const offsetY = Math.min(Math.max(0, start.y - rect.top), rect.height);
  const move = (point: ReorderPoint) => {
    const insetX = rect.width + 12 <= window.innerWidth ? 6 : 0;
    const insetY = rect.height + 12 <= window.innerHeight ? 6 : 0;
    const maxLeft = Math.max(insetX, window.innerWidth - rect.width - insetX);
    const maxTop = Math.max(insetY, window.innerHeight - rect.height - insetY);
    const left = Math.min(maxLeft, Math.max(insetX, point.x - offsetX));
    const top = Math.min(maxTop, Math.max(insetY, point.y - offsetY));
    lift.style.left = `${left}px`;
    lift.style.top = `${top}px`;
  };
  move(start);
  return { lift, move };
}

function suppressDragClick() {
  const stop = (event: MouseEvent) => {
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  window.addEventListener('click', stop, { capture: true, once: true });
  window.setTimeout(() => window.removeEventListener('click', stop, true), 0);
}

export function pointerReorder(node: HTMLElement, initialOptions: PointerReorderOptions) {
  let options = initialOptions;

  function keyDown(event: KeyboardEvent) {
    if (options.disabled || activeCleanup || !event.altKey) return;
    const direction = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0;
    if (direction === 0) return;
    const grip =
      event.target instanceof Element ? event.target.closest('[data-reorder-handle]') : null;
    if (options.gripOnly && (!grip || !node.contains(grip))) return;
    if (!options.gripOnly && (!grip || !node.contains(grip)) && event.target !== node) return;

    const entries = reorderElements(options.group);
    const from = entries.findIndex((entry) => entry.id === options.id);
    const target = entries[from + direction];
    if (from < 0 || !target) return;
    event.preventDefault();
    event.stopPropagation();
    options.onStart?.();
    options.onReorder(target.id);
    options.onEnd?.(true);
    announceMove(options, target.id);
    queueMicrotask(() => {
      const source = reorderElements(options.group).find(
        (entry) => entry.id === options.id,
      )?.element;
      (focusTargetFor(source) ?? focusTargetFor(target.element))?.focus();
    });
  }

  function pointerDown(event: PointerEvent) {
    if (options.disabled || activeCleanup || (event.pointerType !== 'touch' && event.button !== 0))
      return;
    const grip =
      event.target instanceof Element ? event.target.closest('[data-reorder-handle]') : null;
    const touchGrip =
      event.target instanceof Element ? event.target.closest('[data-reorder-touch-handle]') : null;
    if (options.gripOnly && (!grip || !node.contains(grip))) return;
    if (
      options.touchGripOnly &&
      event.pointerType === 'touch' &&
      (!touchGrip || !node.contains(touchGrip))
    )
      return;
    if (!options.gripOnly && !grip && interactiveTarget(event.target, node)) return;

    const pointerId = event.pointerId;
    const start = { x: event.clientX, y: event.clientY };
    let dragging = false;
    let moved = false;
    let currentSource: HTMLElement | null = null;
    let lift: ReturnType<typeof makeLift> | null = null;
    let lastTarget: string | null = null;
    let lastPoint = start;
    let scrollFrame = 0;
    const scroller = scrollParent(node);

    const syncSource = () => {
      const next = reorderElements(options.group).find((entry) => entry.id === options.id)?.element;
      if (next === currentSource) return;
      currentSource?.classList.remove('reorder-source');
      currentSource = next ?? null;
      currentSource?.classList.add('reorder-source');
    };

    const cleanup = (cancelled = false, restoreFocus = false) => {
      window.removeEventListener('pointermove', pointerMove, true);
      window.removeEventListener('pointerup', pointerEnd, true);
      window.removeEventListener('pointercancel', pointerEnd, true);
      window.removeEventListener('blur', cancelGesture);
      window.removeEventListener('keydown', cancelFromKeyboard, true);
      window.cancelAnimationFrame(scrollFrame);
      try {
        if (node.hasPointerCapture(pointerId)) node.releasePointerCapture(pointerId);
      } catch {
        // The source can be replaced while crossing between keyed sections.
      }
      currentSource?.classList.remove('reorder-source');
      lift?.lift.remove();
      document.documentElement.classList.remove('is-reordering');
      activeCleanup = null;
      if (dragging) {
        suppressDragClick();
        options.onEnd?.(moved, cancelled);
        if (cancelled) announce(`${options.label ?? 'Item'} move cancelled.`);
        else if (moved) announceMove(options, lastTarget);
        if (restoreFocus) {
          queueMicrotask(() => {
            const source = reorderElements(options.group).find(
              (entry) => entry.id === options.id,
            )?.element;
            focusTargetFor(source)?.focus();
          });
        }
      }
    };

    const cancelGesture = () => cleanup(true);
    const cancelFromKeyboard = (keyEvent: KeyboardEvent) => {
      if (!dragging || keyEvent.key !== 'Escape') return;
      keyEvent.preventDefault();
      keyEvent.stopPropagation();
      cleanup(true, true);
    };

    const reorderAt = (point: ReorderPoint) => {
      syncSource();
      const entries = reorderElements(options.group);
      const target = reorderTargetAt(
        point,
        entries.map(({ id, rect }) => ({
          id,
          top: rect.top,
          right: rect.right,
          bottom: rect.bottom,
          left: rect.left,
        })),
        options.id,
      );
      if (!target) {
        lastTarget = null;
        return;
      }
      if (target === lastTarget) return;
      lastTarget = target;
      moved = true;
      options.onReorder(target);
      hapticSnap(event.pointerType);
    };

    const autoScroll = () => {
      if (!dragging || !scroller) return;
      const delta = edgeScrollDelta(scroller, lastPoint);
      if (delta !== 0) {
        const before = scroller.scrollTop;
        scroller.scrollTop += delta;
        if (scroller.scrollTop !== before) reorderAt(lastPoint);
      }
      scrollFrame = window.requestAnimationFrame(autoScroll);
    };

    const pointerMove = (moveEvent: PointerEvent) => {
      if (moveEvent.pointerId !== pointerId) return;
      lastPoint = { x: moveEvent.clientX, y: moveEvent.clientY };
      if (!dragging) {
        const distance = Math.hypot(moveEvent.clientX - start.x, moveEvent.clientY - start.y);
        if (distance < START_DISTANCE) return;
        dragging = true;
        const previewSource = options.preview?.() ?? node;
        lift = makeLift(previewSource, start);
        lift.move({ x: moveEvent.clientX, y: moveEvent.clientY });
        document.documentElement.classList.add('is-reordering');
        syncSource();
        options.onStart?.();
        scrollFrame = window.requestAnimationFrame(autoScroll);
        try {
          node.setPointerCapture(pointerId);
        } catch {
          // Window listeners still own the gesture when pointer capture is unavailable.
        }
      }

      moveEvent.preventDefault();
      lift?.move(lastPoint);
      reorderAt(lastPoint);
    };

    const pointerEnd = (endEvent: PointerEvent) => {
      if (endEvent.pointerId !== pointerId) return;
      if (dragging) endEvent.preventDefault();
      cleanup(endEvent.type === 'pointercancel');
    };

    window.addEventListener('pointermove', pointerMove, { capture: true, passive: false });
    window.addEventListener('pointerup', pointerEnd, { capture: true, passive: false });
    window.addEventListener('pointercancel', pointerEnd, { capture: true, passive: false });
    window.addEventListener('blur', cancelGesture);
    window.addEventListener('keydown', cancelFromKeyboard, true);
    activeCleanup = cancelGesture;
  }

  node.addEventListener('pointerdown', pointerDown);
  node.addEventListener('keydown', keyDown);
  return {
    update(nextOptions: PointerReorderOptions) {
      options = nextOptions;
    },
    destroy() {
      node.removeEventListener('pointerdown', pointerDown);
      node.removeEventListener('keydown', keyDown);
    },
  };
}
