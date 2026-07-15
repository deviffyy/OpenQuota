import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import QuotaMetric from './QuotaMetric.svelte';
import type { QuotaWindow } from './types';

const now = Date.parse('2026-07-10T12:00:00Z');
const periodSeconds = 10_000;

afterEach(cleanup);

function quota(usedPercent: number, elapsedFraction = 0.5): QuotaWindow {
  return {
    id: 'weekly',
    label: 'Weekly',
    usedPercent,
    format: 'percent',
    usedValue: null,
    limitValue: null,
    periodSeconds,
    resetsAt: new Date(now + (1 - elapsedFraction) * periodSeconds * 1000).toISOString(),
  };
}

function show(value: QuotaWindow, onToggleReset = vi.fn(), isSessionWindow = false) {
  return {
    onToggleReset,
    ...render(QuotaMetric, {
      quota: value,
      now,
      usageDisplay: 'left',
      resetDisplay: 'countdown',
      timeFormat: 'system',
      alwaysShowPacing: false,
      isSessionWindow,
      onToggleUsage: vi.fn(),
      onToggleReset,
    }),
  };
}

describe('quota pacing presentation', () => {
  it('shows the flame, run-out time, and projection tooltip', async () => {
    const onToggleReset = vi.fn();
    const { container } = show(quota(60), onToggleReset);
    const warning = screen.getByRole('button', { name: 'Limit in 56m' });
    expect(container.querySelector('.pace-warning__icon')).toBeInTheDocument();
    expect(warning).toHaveAttribute('data-tooltip', '~20% over limit at reset');
    expect(container.querySelector('.meter-shell')).toHaveAttribute(
      'data-tooltip',
      '~20% over limit at reset',
    );
    expect(container.querySelector('.meter__fill')).toHaveStyle('--fill-percent: 40%');
    expect(screen.getByRole('button', { name: '40% left' })).toHaveAttribute(
      'data-tooltip',
      '60% used',
    );
    expect(screen.getByRole('button', { name: /Resets in/ })).toHaveAttribute(
      'data-tooltip',
      expect.stringContaining('Resets today at'),
    );
    await fireEvent.click(warning);
    expect(onToggleReset).toHaveBeenCalledOnce();
  });

  it('shows a flame without a misleading time at the exact-limit edge', () => {
    const { container } = show(quota(50));
    expect(screen.getByLabelText('Will reach limit')).toBeInTheDocument();
    expect(screen.queryByText(/Limit in/)).not.toBeInTheDocument();
    expect(container.querySelector('.pace-warning')).toHaveAttribute(
      'data-tooltip',
      '~100% used at reset',
    );
  });

  it('shows the rounded spare copy and projected-use tooltip when close', () => {
    show(quota(46));
    expect(screen.getByText('~8% spare')).toHaveAttribute('data-tooltip', '~92% used at reset');
  });

  it('frames the even-pace tick in the selected remaining mode', () => {
    const { container } = show(quota(30, 0.25));
    expect(container.querySelector('.meter__pace')).toHaveStyle('--pace-percent: 75%');
  });

  it('shows an unused rolling session as not started without pacing decoration', () => {
    const { container } = show(quota(0), vi.fn(), true);
    expect(screen.getByText('Not started')).toHaveAttribute(
      'data-tooltip',
      'Sessions start after you send your first message.',
    );
    expect(container.querySelector('.pace-warning')).not.toBeInTheDocument();
    expect(container.querySelector('.meter-shell')).not.toHaveAttribute('data-tooltip');
  });

  it('renders request quotas as counts instead of percentages', () => {
    show({
      ...quota(24),
      id: 'requests',
      label: 'Requests',
      format: 'count',
      usedValue: 120,
      limitValue: 500,
    });
    expect(screen.getByRole('button', { name: '380 requests left' })).toHaveAttribute(
      'data-tooltip',
      '120 requests used',
    );
  });
});
