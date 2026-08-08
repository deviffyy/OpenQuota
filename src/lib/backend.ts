import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  AppSettings,
  BootstrapState,
  ProviderApiKeyState,
  ResetClaimOutcome,
  SettingsViewState,
  UpdateProgress,
  UpdateStatus,
  UsageViewState,
} from './types';

type StopListening = () => void;
type PayloadHandler<T> = (payload: T) => void;
export type PanelResizeEdge = 'top' | 'bottom';
export type PanelHeightMode = 'automatic' | 'manual';

function onEvent<T>(name: string, handler: PayloadHandler<T>): Promise<StopListening> {
  return listen<T>(name, (event) => handler(event.payload));
}

export function getBootstrapState() {
  return invoke<BootstrapState>('get_bootstrap_state');
}

export function refreshUsage() {
  return invoke<UsageViewState>('refresh_usage');
}

export function refreshProviderUsage(providerId: string) {
  return invoke<UsageViewState>('refresh_provider_usage', { providerId });
}

export function claimCodexResetCredit(expiresAt: string, redeemRequestId: string) {
  return invoke<ResetClaimOutcome>('claim_codex_reset_credit', { expiresAt, redeemRequestId });
}

export function openProviderLink(providerId: string, linkIndex: number) {
  return invoke<void>('open_provider_link', { providerId, linkIndex });
}

export function getProviderApiKeyState(providerId: string) {
  return invoke<ProviderApiKeyState | null>('get_provider_api_key_state', { providerId });
}

export function saveProviderApiKey(providerId: string, apiKey: string) {
  return invoke<ProviderApiKeyState>('save_provider_api_key', { providerId, apiKey });
}

export function deleteProviderApiKey(providerId: string) {
  return invoke<ProviderApiKeyState>('delete_provider_api_key', { providerId });
}

export function getAppSettings() {
  return invoke<SettingsViewState>('get_app_settings');
}

export function saveAppSettings(settings: AppSettings, expectedAccountRevision: number) {
  return invoke<SettingsViewState>('save_app_settings', { settings, expectedAccountRevision });
}

export function resetCustomization() {
  return invoke<SettingsViewState>('reset_customization');
}

export function resetProviderCustomization(providerId: string) {
  return invoke<SettingsViewState>('reset_provider_customization', { providerId });
}

export function requestNotificationPermission() {
  return invoke<SettingsViewState>('request_notification_permission');
}

export function openNotificationSettings() {
  return invoke<void>('open_notification_settings');
}

export function getLogPath() {
  return invoke<string>('get_log_path');
}

export function openLogFolder() {
  return invoke<void>('open_log_folder');
}

export function dismissMainWindow() {
  return invoke<void>('dismiss_main_window');
}

export function getPanelResizeEdge() {
  return invoke<PanelResizeEdge>('get_panel_resize_edge');
}

export function getPanelHeightMode() {
  return invoke<PanelHeightMode>('get_panel_height_mode');
}

export function fitPanelToContent(height: number) {
  return invoke<boolean>('fit_panel_to_content', { height });
}

export function setPanelHeightAutomatic() {
  return invoke<void>('set_panel_height_automatic');
}

export function setPanelHeightManual() {
  return invoke<void>('set_panel_height_manual');
}

export function beginPanelResize() {
  return invoke<PanelResizeEdge>('begin_panel_resize');
}

export function lockPanelResizeAxis() {
  return invoke<void>('lock_panel_resize_axis');
}

export function quitApplication() {
  return invoke<void>('quit_app');
}

export function checkForApplicationUpdates() {
  return invoke<UpdateStatus>('check_for_updates');
}

export function installApplicationUpdate() {
  return invoke<void>('install_update');
}

export function openUpdatePage() {
  return invoke<void>('open_update_page');
}

export function onUsageState(handler: PayloadHandler<UsageViewState>) {
  return onEvent('usage-state', handler);
}

export function onSettingsState(handler: PayloadHandler<SettingsViewState>) {
  return onEvent('settings-state', handler);
}

export function onOpenScreen(handler: PayloadHandler<string>) {
  return onEvent('open-screen', handler);
}

export function onMainWindowHidden(handler: PayloadHandler<void>) {
  return onEvent('main-window-hidden', handler);
}

export function onUpdateProgress(handler: PayloadHandler<UpdateProgress>) {
  return onEvent('update-progress', handler);
}
