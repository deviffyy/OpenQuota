export type MetricKind = 'quota' | 'quotaOrValue' | 'trend' | 'usage' | 'value';

export interface MetricDefinition {
  id: string;
  label: string;
  kind: MetricKind;
  sourceId?: string;
  pinnable: boolean;
}

export const metricDefinitions: MetricDefinition[] = [
  { id: 'claude.session', label: 'Session', kind: 'quota', sourceId: 'session', pinnable: true },
  { id: 'claude.weekly', label: 'Weekly', kind: 'quota', sourceId: 'weekly', pinnable: true },
  { id: 'claude.sonnet', label: 'Sonnet', kind: 'quota', sourceId: 'sonnet', pinnable: true },
  { id: 'claude.fable', label: 'Fable', kind: 'quota', sourceId: 'fable', pinnable: true },
  {
    id: 'claude.extra',
    label: 'Extra Usage',
    kind: 'quotaOrValue',
    sourceId: 'extra',
    pinnable: true,
  },
  { id: 'claude.trend', label: 'Usage Trend', kind: 'trend', pinnable: false },
  { id: 'claude.today', label: 'Today', kind: 'usage', sourceId: 'today', pinnable: true },
  {
    id: 'claude.yesterday',
    label: 'Yesterday',
    kind: 'usage',
    sourceId: 'yesterday',
    pinnable: true,
  },
  {
    id: 'claude.last30',
    label: 'Last 30 Days',
    kind: 'usage',
    sourceId: 'last30Days',
    pinnable: true,
  },
  { id: 'codex.session', label: 'Session', kind: 'quota', sourceId: 'session', pinnable: true },
  { id: 'codex.weekly', label: 'Weekly', kind: 'quota', sourceId: 'weekly', pinnable: true },
  { id: 'codex.spark', label: 'Spark', kind: 'quota', sourceId: 'spark', pinnable: true },
  {
    id: 'codex.sparkWeekly',
    label: 'Spark Weekly',
    kind: 'quota',
    sourceId: 'sparkWeekly',
    pinnable: true,
  },
  { id: 'codex.trend', label: 'Usage Trend', kind: 'trend', pinnable: false },
  {
    id: 'codex.credits',
    label: 'Extra Usage',
    kind: 'value',
    sourceId: 'credits',
    pinnable: true,
  },
  {
    id: 'codex.rateLimitResets',
    label: 'Rate Limit Resets',
    kind: 'value',
    sourceId: 'rateLimitResets',
    pinnable: true,
  },
  { id: 'codex.today', label: 'Today', kind: 'usage', sourceId: 'today', pinnable: true },
  {
    id: 'codex.yesterday',
    label: 'Yesterday',
    kind: 'usage',
    sourceId: 'yesterday',
    pinnable: true,
  },
  {
    id: 'codex.last30',
    label: 'Last 30 Days',
    kind: 'usage',
    sourceId: 'last30Days',
    pinnable: true,
  },
  {
    id: 'antigravity.geminiPro',
    label: 'Session',
    kind: 'quota',
    sourceId: 'geminiPro',
    pinnable: true,
  },
  {
    id: 'antigravity.geminiWeekly',
    label: 'Weekly',
    kind: 'quota',
    sourceId: 'geminiWeekly',
    pinnable: true,
  },
  { id: 'antigravity.claude', label: 'Claude', kind: 'quota', sourceId: 'claude', pinnable: true },
  {
    id: 'antigravity.claudeWeekly',
    label: 'Claude Weekly',
    kind: 'quota',
    sourceId: 'claudeWeekly',
    pinnable: true,
  },
];

export function providerDisplayName(id: string) {
  if (id === 'claude') return 'Claude';
  if (id === 'antigravity') return 'Antigravity';
  if (id === 'codex') return 'Codex';
  return id;
}

export function metricDefinition(id: string) {
  return metricDefinitions.find((metric) => metric.id === id);
}

export function providerSupportsSpend(providerId: string) {
  return metricDefinitions.some(
    (metric) => metric.id.startsWith(`${providerId}.`) && metric.kind === 'usage',
  );
}
