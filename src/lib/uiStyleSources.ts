import appSource from '../App.svelte?raw';
import customizeDetailSource from './CustomizeProviderDetail.svelte?raw';
import customizeListSource from './CustomizeProviderList.svelte?raw';
import dashboardSource from './Dashboard.svelte?raw';
import iconSource from './Icon.svelte?raw';
import modelUsageDetailSource from './ModelUsageDetail.svelte?raw';
import providerIconSource from './ProviderIcon.svelte?raw';
import quotaMetricSource from './QuotaMetric.svelte?raw';
import selectMenuSource from './SelectMenu.svelte?raw';
import settingsSource from './SettingsScreen.svelte?raw';
import totalSpendSource from './TotalSpend.svelte?raw';
import usageMetricSource from './UsageMetric.svelte?raw';
import usageTrendSource from './UsageTrend.svelte?raw';

const componentSources = [
  appSource,
  customizeDetailSource,
  customizeListSource,
  dashboardSource,
  iconSource,
  modelUsageDetailSource,
  providerIconSource,
  quotaMetricSource,
  selectMenuSource,
  settingsSource,
  totalSpendSource,
  usageMetricSource,
  usageTrendSource,
];

function extractStyleBlocks(source: string): string[] {
  return Array.from(source.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g), (match) => match[1]);
}

export const coLocatedComponentCss = componentSources.flatMap(extractStyleBlocks).join('\n');
