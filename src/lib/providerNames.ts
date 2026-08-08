import type { AppSettings } from './types';

export function canRenameProvider(providerId: string, renamableProviderIds: readonly string[]) {
  return renamableProviderIds.includes(providerId);
}

export function withProviderName(
  settings: AppSettings,
  providerId: string,
  value: string,
): AppSettings {
  const name = value.trim();
  const current = settings.providerNames[providerId]?.trim() ?? '';
  if (name === current) return settings;

  const providerNames = { ...settings.providerNames };
  if (name) providerNames[providerId] = name;
  else delete providerNames[providerId];
  return { ...settings, providerNames };
}
