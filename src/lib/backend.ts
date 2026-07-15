import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  AppSettings,
  BootstrapState,
  SettingsViewState,
  UpdateProgress,
  UpdateStatus,
  UsageViewState,
} from './types';

type StopListening = () => void;
type PayloadHandler<T> = (payload: T) => void;

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

export function openProviderLink(providerId: string, linkIndex: number) {
  return invoke<void>('open_provider_link', { providerId, linkIndex });
}

export function getAppSettings() {
  return invoke<SettingsViewState>('get_app_settings');
}

export function saveAppSettings(settings: AppSettings) {
  return invoke<SettingsViewState>('save_app_settings', { settings });
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

export function resizeMainWindow(height: number) {
  return invoke<void>('resize_main_window', { height });
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

export function onPopupHidden(handler: PayloadHandler<void>) {
  return onEvent('popup-hidden', handler);
}

export function onUpdateProgress(handler: PayloadHandler<UpdateProgress>) {
  return onEvent('update-progress', handler);
}
