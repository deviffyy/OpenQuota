import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ProviderIcon from './ProviderIcon.svelte';
import UsageTrend from './UsageTrend.svelte';

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('native visual contract', () => {
  it.each(['claude', 'codex', 'antigravity'])(
    'packages the exact %s provider icon',
    (providerId) => {
      const { container } = render(ProviderIcon, { providerId });
      const icon = container.querySelector('.provider-icon');
      expect(icon).not.toBeNull();
      const path = icon?.querySelector('path')?.getAttribute('d');
      expect(path?.length).toBeGreaterThan(100);
    },
  );

  it('renders provider marks with their intended brand treatment', () => {
    const claude = render(ProviderIcon, { providerId: 'claude' });
    expect(claude.container.querySelector('path')).toHaveAttribute('fill', '#DE7356');
    cleanup();
    const codex = render(ProviderIcon, { providerId: 'codex' });
    expect(codex.container.querySelector('path')).toHaveAttribute('fill', 'currentColor');
    cleanup();
    const antigravity = render(ProviderIcon, { providerId: 'antigravity' });
    expect(antigravity.container.querySelector('path')).toHaveAttribute('fill', '#4285F4');
  });

  it('uses the shared hover dwell and grace timing for Usage Trend details', async () => {
    vi.useFakeTimers();
    const today = new Date();
    const date = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
    render(UsageTrend, {
      daily: [{ date, tokens: 42_000, estimatedCostUsd: 0.21, estimateComplete: true }],
      sourceNote: 'From your Codex logs (estimated)',
    });
    const chart = screen.getByRole('group', { name: 'Usage trend chart details' });

    await fireEvent.mouseEnter(chart);
    await vi.advanceTimersByTimeAsync(399);
    expect(screen.queryByText('peak 42K tokens')).not.toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(1);
    expect(screen.getByText('peak 42K tokens')).toBeInTheDocument();
    expect(screen.getByText('From your Codex logs (estimated)')).toBeInTheDocument();

    await fireEvent.mouseLeave(chart);
    await vi.advanceTimersByTimeAsync(179);
    expect(screen.getByText('peak 42K tokens')).toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(1);
    expect(screen.queryByText('peak 42K tokens')).not.toBeInTheDocument();
  });

  it('reveals an exact day value when a detail bar is hovered', async () => {
    vi.useFakeTimers();
    const today = new Date();
    const date = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
    const { container } = render(UsageTrend, {
      daily: [{ date, tokens: 42_000, estimatedCostUsd: 0.21, estimateComplete: true }],
      sourceNote: 'From your Codex logs (estimated)',
    });
    await fireEvent.mouseEnter(screen.getByRole('group', { name: 'Usage trend chart details' }));
    await vi.advanceTimersByTimeAsync(400);
    const bars = container.querySelectorAll<HTMLElement>('.trend-detail__bars i');
    await fireEvent.mouseEnter(bars[bars.length - 1]);
    expect(screen.getByText(/· 42K tokens$/)).toBeInTheDocument();
    expect(container.querySelectorAll('.trend-detail__bars i.muted')).toHaveLength(30);
  });
});
