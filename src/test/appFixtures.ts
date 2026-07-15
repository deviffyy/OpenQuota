import { ProviderCatalogIndex } from '../lib/metrics';
import type {
  MetricDefinition,
  ProviderCatalog,
  ProviderViewState,
  SettingsViewState,
  UsageViewState,
} from '../lib/types';

function quota(
  id: string,
  label: string,
  sourceId: string,
  sessionWindow = false,
): MetricDefinition {
  return {
    id,
    label,
    source: { kind: 'quota', sourceId, sessionWindow },
    pinnable: true,
    defaultEnabled: true,
    defaultSection: 'onDemand',
    defaultPinned: false,
    tray: { shortLabel: label.slice(0, 1), suffix: null },
  };
}

function usage(
  id: string,
  label: string,
  period: 'today' | 'yesterday' | 'last30Days',
): MetricDefinition {
  return {
    id,
    label,
    source: { kind: 'usage', period },
    pinnable: true,
    defaultEnabled: true,
    defaultSection: 'onDemand',
    defaultPinned: false,
    tray: { shortLabel: label.slice(0, 1), suffix: null },
  };
}

function trend(id: string): MetricDefinition {
  return {
    id,
    label: 'Usage Trend',
    source: { kind: 'trend' },
    pinnable: false,
    defaultEnabled: true,
    defaultSection: 'alwaysVisible',
    defaultPinned: false,
    tray: null,
  };
}

export const providerCatalog: ProviderCatalog = {
  providers: [
    {
      id: 'claude',
      displayName: 'Claude',
      shortName: 'Cl',
      fallbackEnabled: false,
      localUsageSourceNote: 'From your Claude usage history (estimated)',
      links: [
        { label: 'Status', url: 'https://status.anthropic.com/' },
        { label: 'Dashboard', url: 'https://claude.ai/settings/usage' },
      ],
      metrics: [
        quota('claude.session', 'Session', 'session', true),
        quota('claude.weekly', 'Weekly', 'weekly'),
        quota('claude.sonnet', 'Sonnet', 'sonnet'),
        quota('claude.fable', 'Fable', 'fable'),
        {
          ...quota('claude.extra', 'Extra Usage', 'extra'),
          source: { kind: 'quotaOrValue', sourceId: 'extra', sessionWindow: false },
        },
        trend('claude.trend'),
        usage('claude.today', 'Today', 'today'),
        usage('claude.yesterday', 'Yesterday', 'yesterday'),
        usage('claude.last30', 'Last 30 Days', 'last30Days'),
      ],
    },
    {
      id: 'codex',
      displayName: 'Codex',
      shortName: 'Cx',
      fallbackEnabled: true,
      localUsageSourceNote: 'From your Codex logs (estimated)',
      links: [
        { label: 'Status', url: 'https://status.openai.com/' },
        { label: 'Dashboard', url: 'https://chatgpt.com/codex/settings/usage' },
      ],
      metrics: [
        quota('codex.session', 'Session', 'session'),
        quota('codex.weekly', 'Weekly', 'weekly'),
        quota('codex.spark', 'Spark', 'spark'),
        quota('codex.sparkWeekly', 'Spark Weekly', 'sparkWeekly'),
        trend('codex.trend'),
        {
          ...quota('codex.credits', 'Extra Usage', 'credits'),
          source: { kind: 'value', sourceId: 'credits' },
        },
        {
          ...quota('codex.rateLimitResets', 'Rate Limit Resets', 'rateLimitResets'),
          source: { kind: 'value', sourceId: 'rateLimitResets' },
          tray: { shortLabel: 'R', suffix: 'resets' },
        },
        usage('codex.today', 'Today', 'today'),
        usage('codex.yesterday', 'Yesterday', 'yesterday'),
        usage('codex.last30', 'Last 30 Days', 'last30Days'),
      ],
    },
    {
      id: 'antigravity',
      displayName: 'Antigravity',
      shortName: 'A',
      fallbackEnabled: false,
      localUsageSourceNote: null,
      links: [],
      metrics: [
        quota('antigravity.geminiPro', 'Session', 'geminiPro', true),
        quota('antigravity.geminiWeekly', 'Weekly', 'geminiWeekly'),
        quota('antigravity.claude', 'Claude', 'claude', true),
        quota('antigravity.claudeWeekly', 'Claude Weekly', 'claudeWeekly'),
      ],
    },
  ],
};

export const providerCatalogIndex = new ProviderCatalogIndex(providerCatalog);

export const codexState: ProviderViewState = {
  source: 'live',
  refreshing: false,
  stale: false,
  error: null,
  errorKind: null,
  lastAttemptAt: null,
  snapshot: {
    providerId: 'codex',
    plan: 'Plus',
    refreshedAt: '2026-07-10T10:00:00Z',
    warnings: [],
    quotas: [
      {
        id: 'session',
        label: 'Session',
        usedPercent: 32,
        resetsAt: '2099-01-01T00:00:00Z',
        periodSeconds: 18000,
        format: 'percent',
        usedValue: null,
        limitValue: null,
      },
      {
        id: 'weekly',
        label: 'Weekly',
        usedPercent: 59,
        resetsAt: '2099-01-07T00:00:00Z',
        periodSeconds: 604800,
        format: 'percent',
        usedValue: null,
        limitValue: null,
      },
    ],
    valueMetrics: [
      {
        id: 'rateLimitResets',
        label: 'Rate Limit Resets',
        values: [{ number: 2, kind: 'count', label: 'available' }],
        expiriesAt: ['2099-01-02T00:00:00Z', '2099-01-03T00:00:00Z'],
      },
      {
        id: 'credits',
        label: 'Extra Usage',
        values: [
          { number: 32.84, kind: 'dollars' },
          { number: 821, kind: 'count', label: 'credits' },
        ],
        expiriesAt: [],
      },
    ],
    notices: [],
    usage: {
      today: {
        tokens: 2100000,
        estimatedCostUsd: 3.84,
        costEstimated: true,
        estimateComplete: true,
      },
      yesterday: {
        tokens: 684000,
        estimatedCostUsd: 1.27,
        costEstimated: true,
        estimateComplete: true,
      },
      last30Days: {
        tokens: 3000000,
        estimatedCostUsd: 5.11,
        costEstimated: true,
        estimateComplete: true,
      },
      daily: [
        { date: '2026-07-10', tokens: 2100000, estimatedCostUsd: 3.84, estimateComplete: true },
      ],
      unknownModels: [],
    },
  },
};

export const liveState: UsageViewState = { providers: { codex: codexState } };

export const claudeState: ProviderViewState = {
  source: 'live',
  refreshing: false,
  stale: false,
  error: null,
  errorKind: null,
  lastAttemptAt: null,
  snapshot: {
    providerId: 'claude',
    plan: 'Max',
    refreshedAt: '2026-07-11T10:00:00Z',
    warnings: [],
    quotas: [
      {
        id: 'session',
        label: 'Session',
        usedPercent: 20,
        resetsAt: '2099-01-01T00:00:00Z',
        periodSeconds: 18000,
        format: 'percent',
        usedValue: null,
        limitValue: null,
      },
      {
        id: 'extra',
        label: 'Extra Usage',
        usedPercent: 25,
        resetsAt: null,
        periodSeconds: 0,
        format: 'dollars',
        usedValue: 12.5,
        limitValue: 50,
      },
    ],
    valueMetrics: [],
    notices: [],
    usage: { today: null, yesterday: null, last30Days: null, daily: [], unknownModels: [] },
  },
};

export const antigravityState: ProviderViewState = {
  source: 'live',
  refreshing: false,
  stale: false,
  error: null,
  errorKind: null,
  lastAttemptAt: null,
  snapshot: {
    providerId: 'antigravity',
    plan: 'Pro',
    refreshedAt: '2026-07-11T10:00:00Z',
    warnings: [],
    quotas: [
      {
        id: 'geminiPro',
        label: 'Session',
        usedPercent: 0,
        resetsAt: '2099-01-01T00:00:00Z',
        periodSeconds: 18000,
        format: 'percent',
        usedValue: null,
        limitValue: null,
      },
      {
        id: 'geminiWeekly',
        label: 'Weekly',
        usedPercent: 13,
        resetsAt: '2099-01-07T00:00:00Z',
        periodSeconds: 604800,
        format: 'percent',
        usedValue: null,
        limitValue: null,
      },
    ],
    valueMetrics: [],
    notices: [],
    usage: { today: null, yesterday: null, last30Days: null, daily: [], unknownModels: [] },
  },
};

export const settingsState: SettingsViewState = {
  notificationPermission: 'prompt',
  integrationError: null,
  standaloneWindow: false,
  platformSummary: null,
  settings: {
    schemaVersion: 5,
    knownProviderIds: ['claude', 'codex', 'antigravity'],
    showTotalSpend: true,
    theme: 'system',
    density: 'default',
    menuBarStyle: 'text',
    usageDisplay: 'left',
    resetDisplay: 'countdown',
    timeFormat: 'system',
    alwaysShowPacing: false,
    launchAtLogin: false,
    autoCheckUpdates: true,
    dismissedUpdateVersion: null,
    lastUpdateCheckAt: null,
    globalShortcut: null,
    logLevel: 'info',
    notifications: { almostOut: false, cuttingItClose: false, willRunOut: false },
    totalSpendMetric: 'cost',
    totalSpendPeriod: 'today',
    detectionNoticeDismissed: true,
    providers: [
      {
        id: 'codex',
        enabled: true,
        detected: true,
        expanded: false,
        metrics: [
          { id: 'codex.session', enabled: true, section: 'alwaysVisible', pinned: true },
          { id: 'codex.weekly', enabled: true, section: 'alwaysVisible', pinned: true },
          { id: 'codex.spark', enabled: true, section: 'onDemand', pinned: false },
          { id: 'codex.sparkWeekly', enabled: true, section: 'onDemand', pinned: false },
          { id: 'codex.trend', enabled: true, section: 'alwaysVisible', pinned: false },
          { id: 'codex.credits', enabled: true, section: 'onDemand', pinned: false },
          { id: 'codex.rateLimitResets', enabled: true, section: 'onDemand', pinned: false },
          { id: 'codex.today', enabled: true, section: 'onDemand', pinned: false },
          { id: 'codex.yesterday', enabled: true, section: 'onDemand', pinned: false },
          { id: 'codex.last30', enabled: true, section: 'onDemand', pinned: false },
        ],
      },
    ],
  },
};
