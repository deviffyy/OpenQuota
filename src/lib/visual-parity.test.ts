import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ProviderIcon from './ProviderIcon.svelte';
import UsageTrend from './UsageTrend.svelte';

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('OpenUsage visual parity', () => {
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

  it('uses the OpenUsage hover dwell and grace timing for Usage Trend details', async () => {
    vi.useFakeTimers();
    const today = new Date();
    const date = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
    render(UsageTrend, {
      daily: [{ date, tokens: 42_000, estimatedCostUsd: 0.21, estimateComplete: true }],
    });
    const chart = screen.getByRole('group', { name: 'Usage trend chart details' });

    await fireEvent.mouseEnter(chart);
    await vi.advanceTimersByTimeAsync(399);
    expect(screen.queryByText('Peak 42K')).not.toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(1);
    expect(screen.getByText('Peak 42K')).toBeInTheDocument();

    await fireEvent.mouseLeave(chart);
    await vi.advanceTimersByTimeAsync(179);
    expect(screen.getByText('Peak 42K')).toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(1);
    expect(screen.queryByText('Peak 42K')).not.toBeInTheDocument();
  });
});
