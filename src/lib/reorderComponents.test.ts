import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { providerCatalogIndex } from '../test/appFixtures';
import CustomizeProviderDetail from './CustomizeProviderDetail.svelte';
import CustomizeProviderList from './CustomizeProviderList.svelte';
import type { AppSettings } from './types';

afterEach(cleanup);

const settings: AppSettings = {
  schemaVersion: 5,
  knownProviderIds: ['codex', 'claude', 'antigravity'],
  showTotalSpend: false,
  theme: 'system',
  density: 'default',
  menuBarStyle: 'text',
  usageDisplay: 'left',
  resetDisplay: 'countdown',
  timeFormat: 'system',
  alwaysShowPacing: false,
  launchAtLogin: false,
  autoCheckUpdates: true,
  dismissedUpdateVersion: null,
  lastUpdateCheckAt: null,
  globalShortcut: null,
  logLevel: 'info',
  notifications: { almostOut: false, cuttingItClose: false, willRunOut: false },
  totalSpendMetric: 'cost',
  totalSpendPeriod: 'today',
  detectionNoticeDismissed: true,
  providers: [
    {
      id: 'codex',
      enabled: true,
      detected: true,
      expanded: true,
      metrics: [
        { id: 'codex.session', enabled: true, section: 'alwaysVisible', pinned: true },
        { id: 'codex.weekly', enabled: true, section: 'alwaysVisible', pinned: true },
        { id: 'codex.today', enabled: true, section: 'onDemand', pinned: false },
      ],
    },
    {
      id: 'antigravity',
      enabled: false,
      detected: true,
      expanded: false,
      metrics: [],
    },
    {
      id: 'claude',
      enabled: true,
      detected: true,
      expanded: false,
      metrics: [],
    },
  ],
};

function rect(top: number): DOMRect {
  return { top, right: 280, bottom: top + 40, left: 0, width: 280, height: 40 } as DOMRect;
}

async function drag(
  source: HTMLElement,
  handle: HTMLElement,
  target: HTMLElement,
  pointerType: 'mouse' | 'touch' = 'mouse',
) {
  source.getBoundingClientRect = () => rect(0);
  target.getBoundingClientRect = () => rect(40);
  await fireEvent.pointerDown(handle, {
    pointerId: 1,
    pointerType,
    button: 0,
    clientX: 20,
    clientY: 20,
  });
  await fireEvent.pointerMove(window, {
    pointerId: 1,
    pointerType,
    clientX: 20,
    clientY: 52,
  });
  await fireEvent.pointerUp(window, {
    pointerId: 1,
    pointerType,
    clientX: 20,
    clientY: 52,
  });
}

describe('pointer reorder integrations', () => {
  it('keeps dashboard visibility and menu bar stars independent', async () => {
    const onChange = vi.fn();
    const independent = structuredClone(settings);
    const metrics = independent.providers.find((provider) => provider.id === 'codex')!.metrics;
    metrics.find((metric) => metric.id === 'codex.weekly')!.pinned = false;
    const today = metrics.find((metric) => metric.id === 'codex.today')!;
    today.enabled = false;

    const { rerender } = render(CustomizeProviderDetail, {
      settings: independent,
      providerId: 'codex',
      catalog: providerCatalogIndex,
      onChange,
      onReorderStart: vi.fn(),
      onReorderEnd: vi.fn(),
      reducedMotion: false,
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Pin Today' }));
    const starred = onChange.mock.calls.at(-1)![0] as AppSettings;
    const starredToday = starred.providers
      .find((provider) => provider.id === 'codex')!
      .metrics.find((metric) => metric.id === 'codex.today')!;
    expect(starredToday).toMatchObject({ enabled: false, pinned: true });
    expect(screen.getByRole('status')).toHaveTextContent('Starred for menu bar');

    onChange.mockClear();
    await rerender({ settings: starred });
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Show Session' }));
    const hidden = onChange.mock.calls.at(-1)![0] as AppSettings;
    const hiddenSession = hidden.providers
      .find((provider) => provider.id === 'codex')!
      .metrics.find((metric) => metric.id === 'codex.session')!;
    expect(hiddenSession).toMatchObject({ enabled: false, pinned: true });
  });

  it('shakes the denied star control while preserving the two-star cap', async () => {
    const onChange = vi.fn();
    render(CustomizeProviderDetail, {
      settings,
      providerId: 'codex',
      catalog: providerCatalogIndex,
      onChange,
      onReorderStart: vi.fn(),
      onReorderEnd: vi.fn(),
      reducedMotion: false,
    });

    const button = screen.getByRole('button', { name: 'Pin Today' });
    const animate = vi.fn();
    Object.defineProperty(button, 'animate', { configurable: true, value: animate });
    await fireEvent.click(button);

    expect(onChange).not.toHaveBeenCalled();
    expect(animate).toHaveBeenCalledOnce();
    expect(screen.getByRole('status')).toHaveTextContent('Up to 2 stars per provider');
  });

  it('reorders enabled providers from the grip and keeps disabled providers at the tail', async () => {
    const onChange = vi.fn();
    const onReorderStart = vi.fn();
    const onReorderEnd = vi.fn();
    render(CustomizeProviderList, {
      settings,
      catalog: providerCatalogIndex,
      onOpen: vi.fn(),
      onChange,
      onReorderStart,
      onReorderEnd,
      onSettings: vi.fn(),
      reducedMotion: false,
    });

    const rows = screen.getAllByRole('listitem');
    const codex = rows.find((row) => row.textContent?.includes('Codex'))!;
    const claude = rows.find((row) => row.textContent?.includes('Claude'))!;
    await drag(codex, codex.querySelector('[data-reorder-handle]')!, claude);

    expect(onReorderStart).toHaveBeenCalledOnce();
    expect(onReorderEnd).toHaveBeenCalledWith(true, false);
    expect(
      onChange.mock.calls[0][0].providers.map((provider: { id: string }) => provider.id),
    ).toEqual(['claude', 'codex', 'antigravity']);
  });

  it('offers the same provider reorder through Alt+Arrow keyboard controls', async () => {
    const onChange = vi.fn();
    const onReorderStart = vi.fn();
    const onReorderEnd = vi.fn();
    render(CustomizeProviderList, {
      settings,
      catalog: providerCatalogIndex,
      onOpen: vi.fn(),
      onChange,
      onReorderStart,
      onReorderEnd,
      onSettings: vi.fn(),
      reducedMotion: false,
    });

    const handle = screen.getByRole('button', { name: 'Move Codex' });
    handle.focus();
    await fireEvent.keyDown(handle, { key: 'ArrowDown', altKey: true });

    expect(onReorderStart).toHaveBeenCalledOnce();
    expect(onReorderEnd).toHaveBeenCalledWith(true);
    expect(
      onChange.mock.calls[0][0].providers.map((provider: { id: string }) => provider.id),
    ).toEqual(['claude', 'codex', 'antigravity']);
  });

  it('moves a metric across Customize sections through the same pointer engine', async () => {
    const onChange = vi.fn();
    render(CustomizeProviderDetail, {
      settings,
      providerId: 'codex',
      catalog: providerCatalogIndex,
      onChange,
      onReorderStart: vi.fn(),
      onReorderEnd: vi.fn(),
      reducedMotion: false,
    });

    const weekly = screen.getByText('Weekly').closest('.customize-metric-row') as HTMLElement;
    const today = screen.getByText('Today').closest('.customize-metric-row') as HTMLElement;
    await drag(weekly, weekly.querySelector('[data-reorder-handle]')!, today, 'touch');

    const changed = onChange.mock.calls[0][0] as AppSettings;
    const metrics = changed.providers.find((provider) => provider.id === 'codex')!.metrics;
    expect(metrics.find((metric) => metric.id === 'codex.weekly')?.section).toBe('onDemand');
    expect(metrics.map((metric) => metric.id)).toEqual([
      'codex.session',
      'codex.today',
      'codex.weekly',
    ]);
  });
});
