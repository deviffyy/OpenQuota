import { describe, expect, it } from 'vitest';
import { formatReset, projectPace } from './pacing';
import type { QuotaWindow } from './types';

const now = new Date('2026-07-10T12:00:00Z').getTime();
function quota(usedPercent: number, elapsedFraction: number): QuotaWindow {
  const periodSeconds = 10_000;
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

describe('quota pacing', () => {
  it('distinguishes healthy, close, and running-out projections', () => {
    expect(projectPace(quota(30, 0.5), now).severity).toBe('healthy');
    expect(projectPace(quota(46, 0.5), now).severity).toBe('close');
    expect(projectPace(quota(60, 0.5), now).severity).toBe('runningOut');
  });

  it('supports countdown and exact reset modes', () => {
    const reset = new Date(now + 90 * 60_000).toISOString();
    expect(formatReset(reset, now, 'countdown')).toBe('Resets in 1h 30m');
    expect(formatReset(reset, now, 'exact')).toContain('Resets today at');
  });

  it('honors explicit 12-hour and 24-hour clock preferences', () => {
    const reset = new Date('2026-07-10T18:30:00Z').toISOString();
    const twelveHour = formatReset(reset, now, 'exact', 'twelveHour');
    const twentyFourHour = formatReset(reset, now, 'exact', 'twentyFourHour');
    const dayPeriod = new Intl.DateTimeFormat([], { hour: '2-digit', hour12: true })
      .formatToParts(new Date(reset))
      .find((part) => part.type === 'dayPeriod')?.value;
    expect(dayPeriod).toBeTruthy();
    expect(twelveHour).toContain(dayPeriod);
    expect(twentyFourHour).not.toContain(dayPeriod);
  });
});
