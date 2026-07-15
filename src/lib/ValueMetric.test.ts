import { render, screen } from '@testing-library/svelte';
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

  it('shows sorted reset expiries and distinguishes count-only fallback', () => {
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

    expect(screen.getByText('2 available')).toHaveAttribute(
      'data-tooltip',
      'Resets expire in:\n1. 1h 30m\n2. 3h',
    );

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
    expect(screen.getByText('3 available')).toHaveAttribute(
      'data-tooltip',
      'Expiry times unavailable',
    );
  });
});
