import type {
  ModelUsageBreakdown,
  ModelUsageEntry,
  UsageHistory,
  UsagePeriod,
  UsageViewState,
} from './types';

export interface AnalyticsReport {
  tokens: number;
  estimatedCostUsd: number;
  unpricedEvents: number;
  completedTasks: number;
  pricedTasks: number;
  averageCostUsd: number | null;
  timedTasks: number;
  averageSeconds: number | null;
  filesRead: number;
  unreadableFiles: number;
  malformedLines: number;
  earliestEvent: string | null;
  modelBreakdown: ModelUsageBreakdown | null;
  unknownModels: string[];
}

export interface PremiumSeatComparison {
  monthlySeatCostUsd: number;
  proratedSeatCostUsd: number;
  combinedCostUsd: number;
  apiEquivalentUsd: number;
  valueDifferenceUsd: number;
  valueRatio: number | null;
}

export function emptyAnalyticsReport(): AnalyticsReport {
  return {
    tokens: 0,
    estimatedCostUsd: 0,
    unpricedEvents: 0,
    completedTasks: 0,
    pricedTasks: 0,
    averageCostUsd: null,
    timedTasks: 0,
    averageSeconds: null,
    filesRead: 0,
    unreadableFiles: 0,
    malformedLines: 0,
    earliestEvent: null,
    modelBreakdown: null,
    unknownModels: [],
  };
}

export function mergeAnalyticsReports(reports: AnalyticsReport[]): AnalyticsReport {
  const merged = emptyAnalyticsReport();
  let pricedTaskCost = 0;
  let timedTaskSeconds = 0;
  for (const report of reports) {
    merged.tokens += report.tokens;
    merged.estimatedCostUsd += report.estimatedCostUsd;
    merged.unpricedEvents += report.unpricedEvents;
    merged.completedTasks += report.completedTasks;
    merged.pricedTasks += report.pricedTasks;
    merged.timedTasks += report.timedTasks;
    merged.filesRead += report.filesRead;
    merged.unreadableFiles += report.unreadableFiles;
    merged.malformedLines += report.malformedLines;
    merged.unknownModels.push(...report.unknownModels);
    if (report.averageCostUsd !== null) {
      pricedTaskCost += report.averageCostUsd * report.pricedTasks;
    }
    if (report.averageSeconds !== null) {
      timedTaskSeconds += report.averageSeconds * report.timedTasks;
    }
    if (
      report.earliestEvent &&
      (!merged.earliestEvent || Date.parse(report.earliestEvent) < Date.parse(merged.earliestEvent))
    ) {
      merged.earliestEvent = report.earliestEvent;
    }
  }
  merged.averageCostUsd = merged.pricedTasks > 0 ? pricedTaskCost / merged.pricedTasks : null;
  merged.averageSeconds = merged.timedTasks > 0 ? timedTaskSeconds / merged.timedTasks : null;
  merged.unknownModels = [...new Set(merged.unknownModels)].sort();
  merged.modelBreakdown = mergeModelBreakdowns(
    reports.map((report) => report.modelBreakdown).filter((value) => value !== null),
  );
  return merged;
}

function mergeModelBreakdowns(breakdowns: ModelUsageBreakdown[]): ModelUsageBreakdown | null {
  if (!breakdowns.length) return null;
  const models = new Map<string, ModelUsageEntry>();
  for (const breakdown of breakdowns) {
    for (const model of breakdown.models) {
      const key = model.model.toLowerCase();
      const current = models.get(key);
      models.set(key, {
        model: current?.model ?? model.model,
        totalTokens: (current?.totalTokens ?? 0) + model.totalTokens,
        costUsd:
          current?.costUsd === null || model.costUsd === null
            ? null
            : (current?.costUsd ?? 0) + model.costUsd,
        variants: null,
      });
    }
  }
  return {
    models: [...models.values()].sort(
      (left, right) =>
        (right.costUsd ?? -1) - (left.costUsd ?? -1) ||
        right.totalTokens - left.totalTokens ||
        left.model.localeCompare(right.model),
    ),
    sourceNote:
      breakdowns.length === 1
        ? breakdowns[0].sourceNote
        : 'From your selected local provider usage histories (estimated)',
  };
}

export function analyticsReportToUsagePeriod(report: AnalyticsReport): UsagePeriod {
  const incomplete =
    report.unpricedEvents > 0 || report.unreadableFiles > 0 || report.malformedLines > 0;
  return {
    tokens: report.tokens,
    estimatedCostUsd:
      report.unpricedEvents > 0 && report.estimatedCostUsd <= 0 ? null : report.estimatedCostUsd,
    costEstimated: true,
    estimateComplete: !incomplete,
    modelBreakdown: report.modelBreakdown,
    unknownModels: report.unknownModels,
  };
}

export function analyticsFromDailyHistory(
  history: UsageHistory,
  startWallTime: string,
  endWallTime: string,
): AnalyticsReport | null {
  const startDay = startWallTime.slice(0, 10);
  const endDay = endWallTime.slice(0, 10);
  const endIncludesDay = endWallTime.slice(11, 16) !== '00:00';
  const days = history.daily.filter(
    (day) => day.date >= startDay && (day.date < endDay || (endIncludesDay && day.date === endDay)),
  );
  if (!days.length) return null;
  const report = emptyAnalyticsReport();
  for (const day of days) {
    report.tokens += day.tokens;
    if (day.estimatedCostUsd === null) {
      if (day.tokens > 0) report.unpricedEvents += 1;
    } else {
      report.estimatedCostUsd += day.estimatedCostUsd;
    }
  }
  return report;
}

export function relativeMoment(value: string, now: number, future: boolean): string {
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return 'unavailable';
  const difference = Math.max(0, future ? timestamp - now : now - timestamp);
  const minutes = Math.floor(difference / 60000);
  if (minutes < 1) return future ? 'in less than a minute' : 'just now';
  if (minutes < 60) return future ? `in ${minutes} min` : `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return future ? `in ${hours}h` : `${hours}h ago`;
  const days = Math.floor(hours / 24);
  const remainingHours = hours % 24;
  const duration = `${days}d${remainingHours ? ` ${remainingHours}h` : ''}`;
  return future ? `in ${duration}` : `${duration} ago`;
}

const AVERAGE_MONTH_MS = (365.25 / 12) * 86400000;

export function comparePremiumSeats({
  seatCount,
  monthlyPricePerSeatUsd,
  additionalApiCostUsd,
  apiEquivalentUsd,
  from,
  to,
}: {
  seatCount: number;
  monthlyPricePerSeatUsd: number;
  additionalApiCostUsd: number;
  apiEquivalentUsd: number;
  from: string;
  to: string;
}): PremiumSeatComparison {
  const start = Date.parse(from);
  const end = Date.parse(to);
  const values = [seatCount, monthlyPricePerSeatUsd, additionalApiCostUsd, apiEquivalentUsd];
  if (
    !Number.isFinite(start) ||
    !Number.isFinite(end) ||
    start >= end ||
    values.some((value) => !Number.isFinite(value) || value < 0)
  ) {
    throw new Error('The comparison values are invalid.');
  }
  const monthlySeatCostUsd = seatCount * monthlyPricePerSeatUsd;
  const proratedSeatCostUsd = monthlySeatCostUsd * ((end - start) / AVERAGE_MONTH_MS);
  const combinedCostUsd = proratedSeatCostUsd + additionalApiCostUsd;
  return {
    monthlySeatCostUsd,
    proratedSeatCostUsd,
    combinedCostUsd,
    apiEquivalentUsd,
    valueDifferenceUsd: apiEquivalentUsd - combinedCostUsd,
    valueRatio: combinedCostUsd > 0 ? apiEquivalentUsd / combinedCostUsd : null,
  };
}

// A quota boundary is inferred from the provider's window, never from Monday.
export function weeklyStart(
  state: UsageViewState,
  now: number,
  providerId = 'codex',
): string | null {
  const starts = Object.entries(state.providers).flatMap(([id, provider]) => {
    if (id.split('@', 1)[0] !== providerId || provider.stale) return [];
    const quota = provider.snapshot?.quotas.find(
      (q) => q.id === 'weekly' && q.periodSeconds === 604800,
    );
    if (!quota?.resetsAt) return [];
    const reset = Date.parse(quota.resetsAt);
    const start = reset - quota.periodSeconds * 1000;
    return Number.isFinite(reset) && start <= now && now < reset
      ? [new Date(start).toISOString()]
      : [];
  });
  const uniqueStarts = [...new Set(starts)];
  return uniqueStarts.length === 1 ? uniqueStarts[0] : null;
}

interface WallTime {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
}

function parseWallTime(value: string): WallTime | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(value);
  if (!match) return null;
  const [, year, month, day, hour, minute] = match;
  const parsed = {
    year: Number(year),
    month: Number(month),
    day: Number(day),
    hour: Number(hour),
    minute: Number(minute),
  };
  const check = new Date(
    Date.UTC(parsed.year, parsed.month - 1, parsed.day, parsed.hour, parsed.minute),
  );
  return check.getUTCFullYear() === parsed.year &&
    check.getUTCMonth() + 1 === parsed.month &&
    check.getUTCDate() === parsed.day &&
    check.getUTCHours() === parsed.hour &&
    check.getUTCMinutes() === parsed.minute
    ? parsed
    : null;
}

function zonedParts(date: Date, timeZone: string): WallTime {
  const parts = new Intl.DateTimeFormat('en-CA', {
    timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(date);
  const value = (type: Intl.DateTimeFormatPartTypes) =>
    Number(parts.find((part) => part.type === type)?.value);
  return {
    year: value('year'),
    month: value('month'),
    day: value('day'),
    hour: value('hour'),
    minute: value('minute'),
  };
}

function wallEpoch(value: WallTime): number {
  return Date.UTC(value.year, value.month - 1, value.day, value.hour, value.minute);
}

function sameWallTime(a: WallTime, b: WallTime): boolean {
  return (
    a.year === b.year &&
    a.month === b.month &&
    a.day === b.day &&
    a.hour === b.hour &&
    a.minute === b.minute
  );
}

export function detectedTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
}

export function availableTimeZones(): string[] {
  const intl = Intl as typeof Intl & {
    supportedValuesOf?: (key: 'timeZone') => string[];
  };
  const zones = intl.supportedValuesOf?.('timeZone') ?? [];
  return Array.from(new Set([detectedTimeZone(), 'UTC', ...zones])).sort((a, b) =>
    a.localeCompare(b),
  );
}

export function dateTimeInZone(date: Date, timeZone: string): string {
  const value = zonedParts(date, timeZone);
  const pad = (part: number) => String(part).padStart(2, '0');
  return `${value.year}-${pad(value.month)}-${pad(value.day)}T${pad(value.hour)}:${pad(value.minute)}`;
}

export function zonedDateTimeToIso(value: string, timeZone: string): string {
  const desired = parseWallTime(value);
  if (!desired) throw new Error('Enter a valid date and time.');

  // Resolve a wall-clock value without assuming the computer's own time zone.
  let candidate = wallEpoch(desired);
  for (let i = 0; i < 4; i += 1) {
    const actual = zonedParts(new Date(candidate), timeZone);
    const correction = wallEpoch(desired) - wallEpoch(actual);
    candidate += correction;
    if (correction === 0) break;
  }

  // During a fall-back overlap, choose the earliest matching instant. During a
  // spring-forward gap, no matching instant exists and the input is rejected.
  const matches: number[] = [];
  for (let offsetMinutes = -180; offsetMinutes <= 180; offsetMinutes += 15) {
    const instant = candidate + offsetMinutes * 60000;
    if (sameWallTime(zonedParts(new Date(instant), timeZone), desired)) matches.push(instant);
  }
  if (!matches.length) {
    throw new Error(`This time does not exist in ${timeZone} because of daylight saving time.`);
  }
  return new Date(Math.min(...matches)).toISOString();
}

export function formatInZone(value: string, timeZone: string): string {
  return new Intl.DateTimeFormat('en-US', {
    timeZone,
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(value));
}

export function localDateTime(date: Date): string {
  return dateTimeInZone(date, detectedTimeZone());
}

export function rangeFromZonedLocal(
  start: string,
  end: string,
  timeZone: string,
): [string, string] {
  const a = zonedDateTimeToIso(start, timeZone);
  const b = zonedDateTimeToIso(end, timeZone);
  if (Date.parse(a) >= Date.parse(b)) {
    throw new Error('Choose a start time before the end time.');
  }
  return [a, b];
}

export function rangeFromLocal(start: string, end: string): [string, string] {
  return rangeFromZonedLocal(start, end, detectedTimeZone());
}
