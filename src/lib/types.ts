export interface QuotaWindow {
  id: string;
  label: string;
  usedPercent: number;
  resetsAt: string | null;
  periodSeconds: number;
  format: 'percent' | 'dollars' | 'count';
  usedValue: number | null;
  limitValue: number | null;
}

export interface MetricValue {
  number: number;
  kind: 'count' | 'dollars';
  label?: string | null;
}

export interface ValueMetric {
  id: string;
  label: string;
  values: MetricValue[];
  expiriesAt: string[];
}

export interface ProviderNotice {
  id: string;
  title: string;
  message: string;
  tone: 'info' | 'warning';
}

export interface UsagePeriod {
  tokens: number;
  estimatedCostUsd: number | null;
  costEstimated: boolean;
  estimateComplete: boolean;
  modelBreakdown?: ModelUsageBreakdown | null;
  unknownModels?: string[];
}

export interface ModelUsageEntry {
  model: string;
  totalTokens: number;
  costUsd: number | null;
  variants?: ModelUsageVariant[] | null;
}

export interface ModelUsageVariant {
  model: string;
  totalTokens: number;
  costUsd: number | null;
}

export interface ModelUsageBreakdown {
  models: ModelUsageEntry[];
  sourceNote: string;
}

export interface DailyUsage {
  date: string;
  tokens: number;
  estimatedCostUsd: number | null;
  estimateComplete: boolean;
}

export interface UsageHistory {
  today: UsagePeriod | null;
  yesterday: UsagePeriod | null;
  last30Days: UsagePeriod | null;
  daily: DailyUsage[];
  unknownModels: string[];
}

export interface ProviderSnapshot {
  providerId: string;
  plan: string | null;
  quotas: QuotaWindow[];
  valueMetrics: ValueMetric[];
  notices: ProviderNotice[];
  usage: UsageHistory;
  warnings: string[];
  refreshedAt: string;
}

export type ProviderErrorKind =
  | 'authentication'
  | 'permission'
  | 'rateLimited'
  | 'network'
  | 'invalidResponse'
  | 'credentialStorage'
  | 'localData'
  | 'storage'
  | 'internal';

export interface ProviderViewState {
  snapshot: ProviderSnapshot | null;
  source: 'none' | 'cache' | 'live';
  refreshing: boolean;
  stale: boolean;
  error: string | null;
  errorKind: ProviderErrorKind | null;
  lastAttemptAt: string | null;
}

export interface UsageViewState {
  providers: Record<string, ProviderViewState>;
  lastFullRefreshAt?: string | null;
}

export type MetricSection = 'alwaysVisible' | 'onDemand';

export type MetricSource =
  | { kind: 'quota'; sourceId: string; sessionWindow: boolean }
  | { kind: 'quotaOrValue'; sourceId: string; sessionWindow: boolean }
  | { kind: 'value'; sourceId: string }
  | { kind: 'usage'; period: 'today' | 'yesterday' | 'last30Days' }
  | { kind: 'trend' };

export interface TrayMetricDefinition {
  shortLabel: string;
  suffix: string | null;
}

export interface MetricDefinition {
  id: string;
  label: string;
  source: MetricSource;
  pinnable: boolean;
  defaultEnabled: boolean;
  defaultSection: MetricSection;
  defaultPinned: boolean;
  tray: TrayMetricDefinition | null;
}

export interface ProviderLink {
  label: string;
  url: string;
}

export interface ProviderDefinition {
  id: string;
  displayName: string;
  shortName: string;
  fallbackEnabled: boolean;
  localUsageSourceNote: string | null;
  links: ProviderLink[];
  metrics: MetricDefinition[];
}

export interface ProviderCatalog {
  providers: ProviderDefinition[];
}

export interface MetricLayout {
  id: string;
  enabled: boolean;
  section: MetricSection;
  pinned: boolean;
}

export interface ProviderLayout {
  id: string;
  enabled: boolean;
  detected: boolean;
  expanded: boolean;
  metrics: MetricLayout[];
}

export interface NotificationPreferences {
  almostOut: boolean;
  cuttingItClose: boolean;
  willRunOut: boolean;
}

export interface AppSettings {
  schemaVersion: number;
  providers: ProviderLayout[];
  knownProviderIds: string[];
  showTotalSpend: boolean;
  theme: 'system' | 'light' | 'dark';
  density: 'default' | 'compact';
  menuBarStyle: 'text' | 'bars';
  usageDisplay: 'used' | 'left';
  resetDisplay: 'countdown' | 'exact';
  timeFormat: 'system' | 'twelveHour' | 'twentyFourHour';
  alwaysShowPacing: boolean;
  launchAtLogin: boolean;
  autoCheckUpdates: boolean;
  dismissedUpdateVersion: string | null;
  lastUpdateCheckAt: string | null;
  globalShortcut: string | null;
  notifications: NotificationPreferences;
  totalSpendMetric: 'cost' | 'costPerMillion' | 'tokens';
  totalSpendPeriod: 'today' | 'yesterday' | 'last30Days';
  detectionNoticeDismissed: boolean;
}

export interface UpdateStatus {
  available: boolean;
  currentVersion: string;
  version: string | null;
  body: string | null;
  installable: boolean;
  releaseUrl: string;
}

export interface UpdateProgress {
  phase: 'downloading' | 'retrying' | 'installing';
  downloaded: number;
  total: number | null;
  percent: number | null;
}

export interface UpdateFailure {
  code: string;
  message: string;
  action: string;
  retryable: boolean;
}

export interface SettingsViewState {
  settings: AppSettings;
  notificationPermission: 'granted' | 'denied' | 'prompt' | 'unavailable';
  integrationError: string | null;
  standaloneWindow: boolean;
  platformSummary: string | null;
}

export interface BootstrapState {
  usage: UsageViewState;
  settings: SettingsViewState;
  catalog: ProviderCatalog;
}
