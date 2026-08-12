import appSource from '../App.svelte?raw';
import confirmationSheetSource from './ConfirmationSheet.svelte?raw';
import customizeDetailSource from './CustomizeProviderDetail.svelte?raw';
import customizeListSource from './CustomizeProviderList.svelte?raw';
import dashboardSource from './Dashboard.svelte?raw';
import iconSource from './Icon.svelte?raw';
import modelUsageDetailSource from './ModelUsageDetail.svelte?raw';
import providerIconSource from './ProviderIcon.svelte?raw';
import providerApiKeySource from './ProviderApiKeySection.svelte?raw';
import quotaMetricSource from './QuotaMetric.svelte?raw';
import resetCreditsDetailSource from './ResetCreditsDetail.svelte?raw';
import selectMenuSource from './SelectMenu.svelte?raw';
import settingsSource from './SettingsScreen.svelte?raw';
import totalSpendSource from './TotalSpend.svelte?raw';
import usageMetricSource from './UsageMetric.svelte?raw';
import usageTrendSource from './UsageTrend.svelte?raw';
import valueMetricSource from './ValueMetric.svelte?raw';

export const componentSources = [
  appSource,
  confirmationSheetSource,
  customizeDetailSource,
  customizeListSource,
  dashboardSource,
  iconSource,
  modelUsageDetailSource,
  providerIconSource,
  providerApiKeySource,
  quotaMetricSource,
  resetCreditsDetailSource,
  selectMenuSource,
  settingsSource,
  totalSpendSource,
  usageMetricSource,
  usageTrendSource,
  valueMetricSource,
];

function extractStyleBlocks(source: string): string[] {
  return Array.from(source.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g), (match) => match[1]);
}

export const coLocatedComponentCss = componentSources.flatMap(extractStyleBlocks).join('\n');
