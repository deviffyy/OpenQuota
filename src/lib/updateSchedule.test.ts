import { describe, expect, it } from 'vitest';
import {
  automaticUpdateDelay,
  UPDATE_CHECK_INTERVAL_MS,
  UPDATE_STARTUP_DELAY_MS,
} from './updateSchedule';

describe('automatic update schedule', () => {
  const now = Date.parse('2026-07-12T12:00:00Z');

  it('waits briefly on first launch or when the saved value is invalid', () => {
    expect(automaticUpdateDelay(null, now)).toBe(UPDATE_STARTUP_DELAY_MS);
    expect(automaticUpdateDelay('invalid', now)).toBe(UPDATE_STARTUP_DELAY_MS);
  });

  it('honors the six hour interval across restarts', () => {
    const fiveHoursAgo = new Date(now - 5 * 60 * 60 * 1000).toISOString();
    expect(automaticUpdateDelay(fiveHoursAgo, now)).toBe(60 * 60 * 1000);
  });

  it('performs an overdue check after the startup delay', () => {
    const overdue = new Date(now - UPDATE_CHECK_INTERVAL_MS - 1).toISOString();
    expect(automaticUpdateDelay(overdue, now)).toBe(UPDATE_STARTUP_DELAY_MS);
  });
});
