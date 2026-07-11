import type { QuotaWindow } from './types';

export type PaceSeverity = 'level' | 'healthy' | 'close' | 'runningOut' | 'spent';

export interface PaceProjection {
  severity: PaceSeverity;
  projectedUsedPercent: number | null;
  evenPacePercent: number | null;
  runOutAt: number | null;
}

export function projectPace(window: QuotaWindow, now: number): PaceProjection {
  const used = clamp(window.usedPercent, 0, 100);
  if (used >= 99.5) {
    return { severity: 'spent', projectedUsedPercent: 100, evenPacePercent: null, runOutAt: now };
  }
  const reset = window.resetsAt ? new Date(window.resetsAt).getTime() : Number.NaN;
  if (!Number.isFinite(reset) || reset <= now || window.periodSeconds <= 0) return level();
  const periodMs = window.periodSeconds * 1000;
  const start = reset - periodMs;
  const elapsed = Math.max(0, now - start);
  const progress = clamp(elapsed / periodMs, 0, 1);
  if (elapsed < 60_000 || progress < 0.02) return level();
  const projected = used / progress;
  const severity = projected >= 100 ? 'runningOut' : 100 - projected < 10 ? 'close' : 'healthy';
  return {
    severity,
    projectedUsedPercent: projected,
    evenPacePercent: progress * 100,
    runOutAt: projected >= 100 && used > 0 ? start + (elapsed * 100) / used : null,
  };
}

type TimeFormat = 'system' | 'twelveHour' | 'twentyFourHour';

export function formatReset(
  value: string | null,
  now: number,
  mode: 'countdown' | 'exact',
  timeFormat: TimeFormat = 'system',
) {
  if (!value) return 'Reset unavailable';
  const reset = new Date(value).getTime();
  if (!Number.isFinite(reset)) return 'Reset unavailable';
  if (reset <= now) return 'Reset due';
  if (mode === 'exact') return `Resets ${formatExact(reset, now, timeFormat)}`;
  return `Resets in ${formatDuration(reset - now)}`;
}

export function formatLimit(
  value: number | null,
  now: number,
  mode: 'countdown' | 'exact',
  timeFormat: TimeFormat = 'system',
) {
  if (value === null) return 'Limit reached';
  if (value <= now) return 'Limit reached';
  return mode === 'exact'
    ? `Limit ${formatExact(value, now, timeFormat)}`
    : `Limit in ${formatDuration(value - now)}`;
}

function formatExact(value: number, now: number, timeFormat: TimeFormat) {
  const date = new Date(value);
  const day = new Date(now).toDateString() === date.toDateString() ? 'today at ' : '';
  return `${day}${date.toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    hour12: timeFormat === 'system' ? undefined : timeFormat === 'twelveHour',
  })}`;
}

function formatDuration(milliseconds: number) {
  const minutes = Math.max(1, Math.ceil(milliseconds / 60_000));
  const days = Math.floor(minutes / 1_440);
  const hours = Math.floor((minutes % 1_440) / 60);
  const remainder = minutes % 60;
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${remainder}m`;
  return `${remainder}m`;
}

function level(): PaceProjection {
  return { severity: 'level', projectedUsedPercent: null, evenPacePercent: null, runOutAt: null };
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.min(maximum, Math.max(minimum, value));
}
