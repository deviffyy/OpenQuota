import type { ProviderViewState, SettingsViewState, UsageViewState } from '../lib/types';

export const codexState: ProviderViewState = {
  source: 'live',
  refreshing: false,
  stale: false,
  error: null,
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
    usage: {
      today: { tokens: 2100000, estimatedCostUsd: 3.84, estimateComplete: true },
      yesterday: { tokens: 684000, estimatedCostUsd: 1.27, estimateComplete: true },
      last30Days: { tokens: 3000000, estimatedCostUsd: 5.11, estimateComplete: true },
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
    usage: { today: null, yesterday: null, last30Days: null, daily: [], unknownModels: [] },
  },
};

export const antigravityState: ProviderViewState = {
  source: 'live',
  refreshing: false,
  stale: false,
  error: null,
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
    usage: { today: null, yesterday: null, last30Days: null, daily: [], unknownModels: [] },
  },
};

export const settingsState: SettingsViewState = {
  notificationPermission: 'prompt',
  integrationError: null,
  standaloneWindow: false,
  platformSummary: null,
  settings: {
    schemaVersion: 4,
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
          { id: 'codex.trend', enabled: true, section: 'alwaysVisible', pinned: false },
          { id: 'codex.today', enabled: true, section: 'onDemand', pinned: false },
          { id: 'codex.yesterday', enabled: true, section: 'onDemand', pinned: false },
          { id: 'codex.last30', enabled: true, section: 'onDemand', pinned: false },
        ],
      },
    ],
  },
};
