import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import UsageAnalytics from './UsageAnalytics.svelte';
import type { UsageViewState } from './types';
import { dateTimeInZone, detectedTimeZone, type AnalyticsReport } from './analytics';

const mocks = vi.hoisted(() => ({ range: vi.fn(), marker: vi.fn() }));
vi.mock('./backend', () => ({
  getRangeAnalytics: mocks.range,
  getBankedResetMarker: mocks.marker,
  onAnalyticsResetWarning: vi.fn().mockResolvedValue(() => {}),
}));
const empty = { providers: {}, lastFullRefreshAt: null } as UsageViewState;
const providerOptions = [{ id: 'codex', name: 'Codex' }];
function renderAnalytics(viewState = empty, onApply = vi.fn()) {
  return render(UsageAnalytics, { viewState, providers: providerOptions, open: true, onApply });
}
afterEach(cleanup);
const report: AnalyticsReport = {
  tokens: 120,
  estimatedCostUsd: 0.002,
  unpricedEvents: 0,
  completedTasks: 1,
  pricedTasks: 1,
  averageCostUsd: 0.002,
  timedTasks: 1,
  averageSeconds: 60,
  filesRead: 1,
  unreadableFiles: 0,
  malformedLines: 0,
  earliestEvent: '2026-10-01T12:00:00Z',
  modelBreakdown: {
    sourceNote: 'From your Codex logs (estimated)',
    models: [{ model: 'gpt-5', totalTokens: 120, costUsd: 0.002 }],
  },
  unknownModels: [],
};
beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  mocks.range.mockResolvedValue(report);
  mocks.marker.mockResolvedValue(null);
});
describe('UsageAnalytics', () => {
  it('sends explicit custom UTC bounds and provider to the real command wrapper', async () => {
    const onApply = vi.fn();
    renderAnalytics(empty, onApply);
    expect(screen.getByLabelText('Provider')).toHaveValue('all');
    await fireEvent.change(screen.getByLabelText('Local time zone'), {
      target: { value: 'America/Toronto' },
    });
    const inputs = [screen.getByLabelText(/Start/), screen.getByLabelText(/End/)];
    await fireEvent.input(inputs[0], { target: { value: '2026-10-01T12:00' } });
    await fireEvent.input(inputs[1], { target: { value: '2026-10-02T12:00' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Calculate' }));
    await waitFor(() =>
      expect(mocks.range).toHaveBeenCalledWith(
        '2026-10-01T16:00:00.000Z',
        '2026-10-02T16:00:00.000Z',
        'codex',
      ),
    );
    expect((await screen.findAllByText('1.0 min')).length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText('Two-person plans vs API')).toBeInTheDocument();
    expect(screen.getByText('ChatGPT Business Premium 5×')).toBeInTheDocument();
    expect(onApply).toHaveBeenCalledWith(
      expect.objectContaining({
        from: '2026-10-01T16:00:00.000Z',
        to: '2026-10-02T16:00:00.000Z',
        providers: [
          expect.objectContaining({
            id: 'codex',
            period: expect.objectContaining({ tokens: 120, estimatedCostUsd: 0.002 }),
          }),
        ],
      }),
    );
  });
  it('does not invent a banked reset or issue a provider claim', async () => {
    renderAnalytics();
    await fireEvent.change(screen.getByLabelText('Provider'), { target: { value: 'codex' } });
    await fireEvent.change(screen.getByLabelText('Usage since'), {
      target: { value: 'banked' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Calculate' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('No reset is recorded');
    expect(mocks.range).not.toHaveBeenCalled();
  });
  it('uses an observed successful banked reset marker', async () => {
    mocks.marker.mockResolvedValue('2026-10-01T12:00:00Z');
    renderAnalytics();
    await fireEvent.change(screen.getByLabelText('Provider'), { target: { value: 'codex' } });
    await fireEvent.change(screen.getByLabelText('Usage since'), {
      target: { value: 'banked' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Calculate' }));
    await waitFor(() =>
      expect(mocks.range).toHaveBeenCalledWith('2026-10-01T12:00:00Z', expect.any(String), 'codex'),
    );
  });
  it('does not offer a weekly range when the provider exposes no valid boundary', async () => {
    renderAnalytics();
    await fireEvent.change(screen.getByLabelText('Provider'), { target: { value: 'codex' } });
    expect(screen.getByLabelText('Usage since')).not.toHaveTextContent('Last weekly reset');
    expect(mocks.range).not.toHaveBeenCalled();
  });
  it('does not turn missing measurements into zero averages', async () => {
    mocks.range.mockResolvedValue({
      ...report,
      averageCostUsd: null,
      averageSeconds: null,
      pricedTasks: 0,
      timedTasks: 0,
    });
    renderAnalytics();
    await fireEvent.click(screen.getByRole('button', { name: 'Calculate' }));
    await waitFor(() =>
      expect(screen.getAllByText('Unavailable').length).toBeGreaterThanOrEqual(2),
    );
  });
  it('includes a daily-only provider in the custom range without inventing tasks', async () => {
    const cursorState = {
      providers: {
        cursor: {
          snapshot: {
            providerId: 'cursor',
            plan: 'Teams Premium',
            quotas: [],
            valueMetrics: [],
            statusMetrics: [],
            notices: [],
            warnings: [],
            refreshedAt: '2026-10-02T14:00:00Z',
            usage: {
              today: null,
              yesterday: null,
              last30Days: null,
              unknownModels: [],
              daily: [
                {
                  date: '2026-10-01',
                  tokens: 500,
                  estimatedCostUsd: 2.5,
                  estimateComplete: true,
                },
              ],
            },
          },
        },
      },
      lastFullRefreshAt: null,
    } as unknown as UsageViewState;
    render(UsageAnalytics, {
      viewState: cursorState,
      providers: [{ id: 'cursor', name: 'Cursor' }],
      open: true,
    });
    await fireEvent.input(screen.getByLabelText(/Start/), {
      target: { value: '2026-10-01T00:00' },
    });
    await fireEvent.input(screen.getByLabelText(/End/), {
      target: { value: '2026-10-02T00:00' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Calculate' }));
    expect(await screen.findByText('Daily precision')).toBeInTheDocument();
    expect(screen.getByText('Cursor Teams Premium 5×')).toBeInTheDocument();
    expect(mocks.range).not.toHaveBeenCalled();
  });
  it('uses the live weekly boundary for a daily-only provider', async () => {
    const zone = detectedTimeZone();
    const insideDay = dateTimeInZone(new Date(), zone).slice(0, 10);
    const outsideDay = dateTimeInZone(new Date(Date.now() - 6 * 86400000), zone).slice(0, 10);
    const cursorState = {
      providers: {
        cursor: {
          stale: false,
          snapshot: {
            providerId: 'cursor',
            plan: 'Teams Premium',
            quotas: [
              {
                id: 'weekly',
                periodSeconds: 604800,
                resetsAt: new Date(Date.now() + 2 * 86400000).toISOString(),
              },
            ],
            valueMetrics: [],
            statusMetrics: [],
            notices: [],
            warnings: [],
            refreshedAt: new Date().toISOString(),
            usage: {
              today: null,
              yesterday: null,
              last30Days: null,
              unknownModels: [],
              daily: [
                {
                  date: outsideDay,
                  tokens: 100,
                  estimatedCostUsd: 1,
                  estimateComplete: true,
                },
                {
                  date: insideDay,
                  tokens: 900,
                  estimatedCostUsd: 9,
                  estimateComplete: true,
                },
              ],
            },
          },
        },
      },
      lastFullRefreshAt: null,
    } as unknown as UsageViewState;
    render(UsageAnalytics, {
      viewState: cursorState,
      providers: [{ id: 'cursor', name: 'Cursor' }],
      open: true,
    });
    await fireEvent.change(screen.getByLabelText('Provider'), { target: { value: 'cursor' } });
    await fireEvent.change(screen.getByLabelText('Usage since'), {
      target: { value: 'weekly' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Calculate' }));
    expect((await screen.findAllByText('900')).length).toBeGreaterThanOrEqual(1);
    expect(screen.queryByText('1,000')).toBeNull();
    expect(mocks.range).not.toHaveBeenCalled();
  });
});
