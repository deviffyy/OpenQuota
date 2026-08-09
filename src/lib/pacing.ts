import type { QuotaWindow } from './types';
import { getFormatLocale, t } from './i18n';

export type PaceSeverity = 'level' | 'healthy' | 'close' | 'runningOut' | 'spent';

export interface PaceProjection {
  severity: PaceSeverity;
  projectedUsedPercent: number | null;
  evenPacePercent: number | null;
  runOutAt: number | null;
}

export function projectPace(
  window: QuotaWindow,
  now: number,
  isSessionWindow = false,
): PaceProjection {
  const used = clamp(window.usedPercent, 0, 100);
  if (isVisiblySpent(window, used)) {
    return { severity: 'spent', projectedUsedPercent: 100, evenPacePercent: null, runOutAt: now };
  }
  const reset = window.resetsAt ? new Date(window.resetsAt).getTime() : Number.NaN;
  if (!Number.isFinite(reset) || reset <= now || window.periodSeconds <= 0) return level();
  if (isFreshSessionWindow(window, now, isSessionWindow)) return level();
  const periodMs = window.periodSeconds * 1000;
  const start = reset - periodMs;
  const elapsed = Math.max(0, now - start);
  const progress = clamp(elapsed / periodMs, 0, 1);
  if (elapsed < Math.max(60_000, periodMs * 0.01)) return level();
  const projected = used / progress;
  if (projected <= 90) {
    return {
      severity: 'healthy',
      projectedUsedPercent: projected,
      evenPacePercent: progress * 100,
      runOutAt: null,
    };
  }
  if (used < 5) return level();
  if (projected <= 100) {
    const spare = Math.round(100 - projected);
    return {
      severity: spare >= 1 ? 'close' : 'runningOut',
      projectedUsedPercent: projected,
      evenPacePercent: progress * 100,
      runOutAt: null,
    };
  }
  const candidate = start + (elapsed * 100) / used;
  return {
    severity: 'runningOut',
    projectedUsedPercent: projected,
    evenPacePercent: progress * 100,
    runOutAt: candidate > now && candidate < reset ? candidate : null,
  };
}

export function isFreshSessionWindow(window: QuotaWindow, now: number, isSessionWindow: boolean) {
  if (!isSessionWindow || window.usedPercent > 0 || !window.resetsAt) return false;
  const reset = new Date(window.resetsAt).getTime();
  return Number.isFinite(reset) && now < reset;
}

export function paceTooltip(value: PaceProjection) {
  if (value.severity === 'level') return null;
  if (value.severity === 'spent') return t('limitReached');
  const projected = value.projectedUsedPercent;
  if (projected === null) return null;
  if (value.severity === 'healthy')
    return t('paceLeftAtReset', { percent: Math.round(100 - projected) });
  if (value.severity === 'close') return t('paceUsedAtReset', { percent: Math.round(projected) });
  if (projected <= 100) return t('paceUsedAtReset', { percent: 100 });
  return t('paceOverLimitAtReset', { percent: Math.max(1, Math.round(projected - 100)) });
}

type TimeFormat = 'system' | 'twelveHour' | 'twentyFourHour';

export function formatReset(
  value: string | null,
  now: number,
  mode: 'countdown' | 'exact',
  timeFormat: TimeFormat = 'system',
) {
  if (!value) return t('resetUnavailable');
  const reset = new Date(value).getTime();
  if (!Number.isFinite(reset)) return t('resetUnavailable');
  return formatDeadline('reset', reset, now, mode, timeFormat);
}

export function formatLimit(
  value: number | null,
  now: number,
  mode: 'countdown' | 'exact',
  timeFormat: TimeFormat = 'system',
) {
  if (value === null) return t('limitReached');
  return formatDeadline('limit', value, now, mode, timeFormat);
}

export function formatResetDetail(
  value: string,
  now: number,
  mode: 'countdown' | 'exact',
  timeFormat: TimeFormat = 'system',
) {
  const timestamp = new Date(value).getTime();
  if (!Number.isFinite(timestamp)) return t('resetUnavailable');
  const remaining = timestamp - now;
  if (mode === 'countdown') {
    return remaining <= 5 * 60_000 ? t('soon') : formatDuration(remaining);
  }
  const date = new Date(timestamp);
  const time = date.toLocaleTimeString(getFormatLocale(), {
    hour: 'numeric',
    minute: '2-digit',
    hour12: timeFormat === 'system' ? undefined : timeFormat === 'twelveHour',
  });
  const dayDifference = calendarDayDifference(timestamp, now);
  if (dayDifference <= 0) return t('todayAt', { time });
  if (dayDifference === 1) return t('tomorrowAt', { time });
  const day = new Intl.DateTimeFormat(getFormatLocale(), {
    month: 'short',
    day: 'numeric',
  }).format(date);
  return t('dateAt', { date: day, time });
}

function formatDeadline(
  kind: 'reset' | 'limit',
  value: number,
  now: number,
  mode: 'countdown' | 'exact',
  timeFormat: TimeFormat,
) {
  const remaining = value - now;
  if (remaining <= 0 || (mode === 'countdown' && remaining <= 5 * 60_000)) {
    return t(kind === 'reset' ? 'resetsSoon' : 'limitSoon');
  }
  if (mode === 'countdown') {
    return t(kind === 'reset' ? 'resetsIn' : 'limitIn', { duration: formatDuration(remaining) });
  }

  const date = new Date(value);
  const dayDifference = calendarDayDifference(value, now);
  const time = date.toLocaleTimeString(getFormatLocale(), {
    hour: 'numeric',
    minute: '2-digit',
    hour12: timeFormat === 'system' ? undefined : timeFormat === 'twelveHour',
  });
  if (dayDifference <= 0) return t(kind === 'reset' ? 'resetsTodayAt' : 'limitTodayAt', { time });
  if (dayDifference === 1)
    return t(kind === 'reset' ? 'resetsTomorrowAt' : 'limitTomorrowAt', { time });
  const monthDay = new Intl.DateTimeFormat(getFormatLocale(), {
    month: 'short',
    day: 'numeric',
  }).format(date);
  return t(kind === 'reset' ? 'resetsDateAt' : 'limitDateAt', { date: monthDay, time });
}

function formatDuration(milliseconds: number) {
  const minutes = Math.max(1, Math.ceil(milliseconds / 60_000));
  const days = Math.floor(minutes / 1_440);
  const hours = Math.floor((minutes % 1_440) / 60);
  const remainder = minutes % 60;
  if (days > 0) return t('durationDaysHours', { days, hours });
  if (hours > 0)
    return remainder > 0
      ? t('durationHoursMinutes', { hours, minutes: remainder })
      : t('durationHours', { hours });
  return t('durationMinutes', { minutes: remainder });
}

function calendarDayDifference(value: number, now: number) {
  const date = new Date(value);
  const current = new Date(now);
  const dateDay = Date.UTC(date.getFullYear(), date.getMonth(), date.getDate());
  const currentDay = Date.UTC(current.getFullYear(), current.getMonth(), current.getDate());
  return Math.round((dateDay - currentDay) / 86_400_000);
}

function level(): PaceProjection {
  return { severity: 'level', projectedUsedPercent: null, evenPacePercent: null, runOutAt: null };
}

function isVisiblySpent(window: QuotaWindow, usedPercent: number) {
  if (
    window.format === 'dollars' &&
    window.usedValue !== null &&
    window.limitValue !== null &&
    window.limitValue > 0
  ) {
    return Math.round((window.limitValue - window.usedValue) * 100) / 100 <= 0;
  }
  return Math.round(100 - usedPercent) <= 0;
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.min(maximum, Math.max(minimum, value));
}
