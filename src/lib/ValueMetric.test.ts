import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ValueMetric from './ValueMetric.svelte';

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));

describe('ValueMetric', () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
  });
  afterEach(cleanup);
  it('renders combined credit values with an exact tooltip for large balances', () => {
    render(ValueMetric, {
      label: 'Extra Usage',
      metric: {
        id: 'credits',
        label: 'Extra Usage',
        values: [
          { number: 1200, kind: 'dollars', estimated: false },
          { number: 30000, kind: 'count', label: 'credits', estimated: false },
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

  it('marks only value rows that contain an estimated value', () => {
    render(ValueMetric, {
      label: 'Extra Usage',
      metric: {
        id: 'credits',
        label: 'Extra Usage',
        values: [
          { number: 4, kind: 'dollars', estimated: true },
          { number: 100, kind: 'count', label: 'credits', estimated: false },
        ],
        expiriesAt: [],
      },
      now: Date.parse('2026-02-20T16:00:00Z'),
      resetDisplay: 'countdown',
      timeFormat: 'twentyFourHour',
    });

    expect(screen.getByLabelText('Estimated value')).toHaveAttribute(
      'data-tooltip',
      'Estimated locally, so it may differ from billed usage.',
    );
  });

  it('opens a sorted reset-expiry timeline and distinguishes count-only fallback', async () => {
    const { rerender } = render(ValueMetric, {
      label: 'Rate Limit Resets',
      metric: {
        id: 'rateLimitResets',
        label: 'Rate Limit Resets',
        values: [{ number: 2, kind: 'count', label: 'available', estimated: false }],
        expiriesAt: ['2026-02-20T19:00:00Z', '2026-02-20T17:30:00Z'],
      },
      now: Date.parse('2026-02-20T16:00:00Z'),
      resetDisplay: 'countdown',
      timeFormat: 'twentyFourHour',
    });

    const trigger = screen.getByRole('button', { name: 'Rate Limit Resets: 2 available' });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    await fireEvent.click(trigger);
    expect(screen.getByRole('dialog', { name: 'Rate Limit Resets details' })).toBeVisible();
    expect(screen.getByText('1h 30m')).toBeInTheDocument();
    expect(screen.getByText('3h')).toBeInTheDocument();

    rerender({
      label: 'Rate Limit Resets',
      metric: {
        id: 'rateLimitResets',
        label: 'Rate Limit Resets',
        values: [{ number: 3, kind: 'count', label: 'available', estimated: false }],
        expiriesAt: [],
      },
      now: Date.parse('2026-02-20T16:00:00Z'),
      resetDisplay: 'countdown',
      timeFormat: 'twentyFourHour',
    });
    expect(screen.getAllByText('3 available')).toHaveLength(2);
    expect(screen.getByText('Expiry times unavailable')).toBeInTheDocument();
  });

  it('requires confirmation and claims one explicitly selected reset credit', async () => {
    mocks.invoke.mockResolvedValue('success');
    render(ValueMetric, {
      label: 'Rate Limit Resets',
      metric: {
        id: 'rateLimitResets',
        label: 'Rate Limit Resets',
        values: [{ number: 1, kind: 'count', label: 'available', estimated: false }],
        expiriesAt: ['2026-02-20T19:00:00Z'],
      },
      now: Date.parse('2026-02-20T16:00:00Z'),
      resetDisplay: 'countdown',
      timeFormat: 'twentyFourHour',
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Rate Limit Resets: 1 available' }));
    await fireEvent.click(screen.getByRole('button', { name: /Use reset expiring/ }));
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(
      screen.getByText("Immediately reset your usage limits. This can't be undone."),
    ).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Use reset' }));

    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith('claim_codex_reset_credit', {
        expiresAt: '2026-02-20T19:00:00Z',
        redeemRequestId: expect.any(String),
      }),
    );
    expect(await screen.findByText('Reset applied.')).toBeInTheDocument();
  });

  it('keeps reset confirmation open when focus moves into the detail panel', async () => {
    vi.useFakeTimers();
    try {
      render(ValueMetric, {
        label: 'Rate Limit Resets',
        metric: {
          id: 'rateLimitResets',
          label: 'Rate Limit Resets',
          values: [{ number: 1, kind: 'count', label: 'available', estimated: false }],
          expiriesAt: ['2026-02-20T19:00:00Z'],
        },
        now: Date.parse('2026-02-20T16:00:00Z'),
        resetDisplay: 'countdown',
        timeFormat: 'twentyFourHour',
      });

      const trigger = screen.getByRole('button', { name: 'Rate Limit Resets: 1 available' });
      trigger.focus();
      await fireEvent.click(trigger);

      const use = screen.getByRole('button', { name: /Use reset expiring/ });
      use.focus();
      await fireEvent.click(use);
      await vi.advanceTimersByTimeAsync(181);

      expect(screen.getByText('Use this reset?')).toBeInTheDocument();
      expect(screen.getByRole('dialog', { name: 'Rate Limit Resets details' })).toBeVisible();
      expect(mocks.invoke).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it('behaves as an anchored popover and closes with Escape', async () => {
    render(ValueMetric, {
      label: 'Rate Limit Resets',
      metric: {
        id: 'rateLimitResets',
        label: 'Rate Limit Resets',
        values: [{ number: 1, kind: 'count', label: 'available', estimated: false }],
        expiriesAt: ['2026-02-20T19:00:00Z'],
      },
      now: Date.parse('2026-02-20T16:00:00Z'),
      resetDisplay: 'countdown',
      timeFormat: 'twentyFourHour',
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Rate Limit Resets: 1 available' }));
    expect(screen.queryByLabelText('Drag Rate Limit Resets panel')).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Close Rate Limit Resets' }),
    ).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: /Use reset expiring/ }));
    const cancel = screen.getByRole('button', { name: 'Cancel' });
    cancel.focus();
    await fireEvent.keyDown(cancel, { key: 'Escape' });
    expect(screen.queryByText('Use this reset?')).not.toBeInTheDocument();
    const dialog = screen.getByRole('dialog', { name: 'Rate Limit Resets details' });
    expect(dialog).toBeVisible();
    expect(dialog).toHaveFocus();

    await fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(
      screen.queryByRole('dialog', { name: 'Rate Limit Resets details' }),
    ).not.toBeInTheDocument();
  });
});
