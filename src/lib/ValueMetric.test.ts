import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import ValueMetric from './ValueMetric.svelte';

describe('ValueMetric', () => {
  it('renders combined credit values with an exact tooltip for large balances', () => {
    render(ValueMetric, {
      label: 'Extra Usage',
      metric: {
        id: 'credits',
        label: 'Extra Usage',
        values: [
          { number: 1200, kind: 'dollars' },
          { number: 30000, kind: 'count', label: 'credits' },
        ],
        expiriesAt: [],
      },
      now: Date.parse('2026-02-20T16:00:00Z'),
      resetDisplay: 'countdown',
      timeFormat: 'twentyFourHour',
    });

    expect(screen.getByText('$1.2K · 30K credits')).toHaveAttribute(
      'data-tooltip',
      '$1,200.00 · 30,000 credits',
    );
  });

  it('opens a sorted reset-expiry timeline and distinguishes count-only fallback', async () => {
    const { rerender } = render(ValueMetric, {
      label: 'Rate Limit Resets',
      metric: {
        id: 'rateLimitResets',
        label: 'Rate Limit Resets',
        values: [{ number: 2, kind: 'count', label: 'available' }],
        expiriesAt: ['2026-02-20T19:00:00Z', '2026-02-20T17:30:00Z'],
      },
      now: Date.parse('2026-02-20T16:00:00Z'),
      resetDisplay: 'countdown',
      timeFormat: 'twentyFourHour',
    });

    const trigger = screen.getByRole('button', { name: 'Rate Limit Resets: 2 available' });
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
    await fireEvent.click(trigger);
    expect(screen.getByRole('tooltip', { name: 'Rate Limit Resets expiry details' })).toBeVisible();
    expect(screen.getByText('1h 30m')).toBeInTheDocument();
    expect(screen.getByText('3h')).toBeInTheDocument();

    rerender({
      label: 'Rate Limit Resets',
      metric: {
        id: 'rateLimitResets',
        label: 'Rate Limit Resets',
        values: [{ number: 3, kind: 'count', label: 'available' }],
        expiriesAt: [],
      },
      now: Date.parse('2026-02-20T16:00:00Z'),
      resetDisplay: 'countdown',
      timeFormat: 'twentyFourHour',
    });
    expect(screen.getAllByText('3 available')).toHaveLength(2);
    expect(screen.getByText('Expiry times unavailable')).toBeInTheDocument();
  });
});
