import { describe, expect, it } from 'vitest';
import {
  analyticsFromDailyHistory,
  analyticsReportToUsagePeriod,
  comparePremiumSeats,
  dateTimeInZone,
  emptyAnalyticsReport,
  mergeAnalyticsReports,
  rangeFromZonedLocal,
  relativeMoment,
  weeklyStart,
  zonedDateTimeToIso,
} from './analytics';
import type { UsageViewState } from './types';

const now = Date.parse('2026-10-10T12:00:00Z');
function state(stale = false, resetsAt = '2026-10-12T12:00:00Z'): UsageViewState {
  return {
    providers: {
      codex: { stale, snapshot: { quotas: [{ id: 'weekly', periodSeconds: 604800, resetsAt }] } },
    },
  } as unknown as UsageViewState;
}
describe('analytics ranges', () => {
  it('derives real provider window rather than calendar week', () => {
    expect(weeklyStart(state(), now)).toBe('2026-10-05T12:00:00.000Z');
  });
  it('does not invent missing or stale reset boundaries', () => {
    expect(weeklyStart(state(true), now)).toBeNull();
    expect(weeklyStart(state(false, '2026-10-09T12:00:00Z'), now)).toBeNull();
    expect(weeklyStart({ providers: {} } as UsageViewState, now)).toBeNull();
  });
  it('resolves account-scoped providers and rejects ambiguous weekly resets', () => {
    const firstReset = '2026-10-12T12:00:00Z';
    const matching = {
      providers: {
        'claude@first': state(false, firstReset).providers.codex,
        'claude@second': state(false, firstReset).providers.codex,
      },
    } as UsageViewState;
    expect(weeklyStart(matching, now, 'claude')).toBe('2026-10-05T12:00:00.000Z');

    matching.providers['claude@second'].snapshot!.quotas[0].resetsAt = '2026-10-12T13:00:00Z';
    expect(weeklyStart(matching, now, 'claude')).toBeNull();
  });
  it('rejects invalid and reversed custom ranges', () => {
    expect(() => rangeFromZonedLocal('', '', 'UTC')).toThrow();
    expect(() => rangeFromZonedLocal('bad', 'bad', 'UTC')).toThrow();
    expect(() => rangeFromZonedLocal('2026-10-10T12:00', '2026-10-10T11:00', 'UTC')).toThrow();
  });
  it('converts selected IANA zones independently of the computer zone', () => {
    expect(zonedDateTimeToIso('2026-01-10T12:30', 'America/Toronto')).toBe(
      '2026-01-10T17:30:00.000Z',
    );
    expect(zonedDateTimeToIso('2026-07-10T12:30', 'America/Toronto')).toBe(
      '2026-07-10T16:30:00.000Z',
    );
    expect(zonedDateTimeToIso('2026-07-10T12:30', 'Asia/Tokyo')).toBe('2026-07-10T03:30:00.000Z');
  });
  it('rejects a wall time skipped by daylight saving', () => {
    expect(() => zonedDateTimeToIso('2026-03-08T02:30', 'America/Toronto')).toThrow(
      'does not exist',
    );
  });
  it('round trips a displayed instant in the chosen zone', () => {
    const date = new Date('2026-10-10T16:30:00Z');
    const wall = dateTimeInZone(date, 'America/Toronto');
    expect(wall).toBe('2026-10-10T12:30');
    expect(zonedDateTimeToIso(wall, 'America/Toronto')).toBe(date.toISOString());
  });
  it('compares two premium seats, added API, and API-equivalent value', () => {
    const comparison = comparePremiumSeats({
      seatCount: 2,
      monthlyPricePerSeatUsd: 125,
      additionalApiCostUsd: 10,
      apiEquivalentUsd: 400,
      from: '2026-01-01T00:00:00Z',
      to: '2026-01-31T10:30:00Z',
    });
    expect(comparison.monthlySeatCostUsd).toBe(250);
    expect(comparison.proratedSeatCostUsd).toBeCloseTo(250, 8);
    expect(comparison.combinedCostUsd).toBeCloseTo(260, 8);
    expect(comparison.valueDifferenceUsd).toBeCloseTo(140, 8);
    expect(comparison.valueRatio).toBeCloseTo(400 / 260, 8);
  });
  it('aggregates daily-only providers without inventing tasks', () => {
    const report = analyticsFromDailyHistory(
      {
        today: null,
        yesterday: null,
        last30Days: null,
        unknownModels: [],
        daily: [
          { date: '2026-10-01', tokens: 100, estimatedCostUsd: 1, estimateComplete: true },
          { date: '2026-10-02', tokens: 200, estimatedCostUsd: null, estimateComplete: false },
          { date: '2026-10-03', tokens: 300, estimatedCostUsd: 3, estimateComplete: true },
        ],
      },
      '2026-10-01T12:00',
      '2026-10-02T12:00',
    );
    expect(report).toMatchObject({
      tokens: 300,
      estimatedCostUsd: 1,
      unpricedEvents: 1,
      completedTasks: 0,
      averageSeconds: null,
    });
  });
  it('does not display an invented zero cost when every event is unpriced', () => {
    expect(
      analyticsReportToUsagePeriod({
        ...emptyAnalyticsReport(),
        tokens: 500,
        unpricedEvents: 1,
      }),
    ).toMatchObject({ estimatedCostUsd: null, estimateComplete: false });
  });
  it('merges weighted task averages across providers', () => {
    const first = {
      ...emptyAnalyticsReport(),
      pricedTasks: 1,
      timedTasks: 1,
      averageCostUsd: 2,
      averageSeconds: 60,
    };
    const second = {
      ...emptyAnalyticsReport(),
      pricedTasks: 3,
      timedTasks: 3,
      averageCostUsd: 4,
      averageSeconds: 100,
    };
    expect(mergeAnalyticsReports([first, second])).toMatchObject({
      pricedTasks: 4,
      averageCostUsd: 3.5,
      timedTasks: 4,
      averageSeconds: 90,
    });
  });
  it('formats reset and update ages independently of the display zone', () => {
    const now = Date.parse('2026-10-10T12:00:00Z');
    expect(relativeMoment('2026-10-15T15:00:00Z', now, true)).toBe('in 5d 3h');
    expect(relativeMoment('2026-10-10T10:00:00Z', now, false)).toBe('2h ago');
  });
});
