<script lang="ts">
  import type { PanelHeightMode } from './backend';
  import Icon from './Icon.svelte';
  import { t } from './i18n';
  import type { DesktopPlatform } from './platform';
  import SelectMenu from './SelectMenu.svelte';
  import type {
    AppSettings,
    NotificationPreferences,
    SettingsViewState,
    UpdateFailure,
  } from './types';

  interface Props {
    settingsView: SettingsViewState;
    platform: DesktopPlatform;
    panelHeightMode: PanelHeightMode;
    onChange: (settings: AppSettings) => void;
    onPanelHeightModeChange: (mode: PanelHeightMode) => void;
    onRequestNotifications: () => void;
    onOpenNotificationSettings: () => void;
    updateError: UpdateFailure | null;
    checkingUpdate: boolean;
    onCheckForUpdates: () => void;
    onCustomize: () => void;
    onCopyLogPath: () => Promise<void>;
    onOpenLogFolder: () => Promise<void>;
  }
  let {
    settingsView,
    platform,
    panelHeightMode,
    onChange,
    onPanelHeightModeChange,
    onRequestNotifications,
    onOpenNotificationSettings,
    updateError,
    checkingUpdate,
    onCheckForUpdates,
    onCustomize,
    onCopyLogPath,
    onOpenLogFolder,
  }: Props = $props();
  let recording = $state(false);
  let logActionError = $state<string | null>(null);
  const settings = $derived(settingsView.settings);
  const revealLogLabel = $derived(
    platform === 'macos'
      ? t('revealInFinder')
      : platform === 'windows'
        ? t('revealInFileExplorer')
        : t('openContainingFolder'),
  );
  const anyNotificationEnabled = $derived(
    settings.notifications.almostOut ||
      settings.notifications.cuttingItClose ||
      settings.notifications.willRunOut,
  );
  const notificationsNeedAttention = $derived(
    anyNotificationEnabled && settingsView.notificationPermission !== 'granted',
  );

  function patch(value: Partial<AppSettings>) {
    onChange({ ...settings, ...value });
  }
  function patchNotification(key: keyof NotificationPreferences, enabled: boolean) {
    patch({ notifications: { ...settings.notifications, [key]: enabled } });
    if (enabled && settingsView.notificationPermission === 'prompt') onRequestNotifications();
  }
  async function copyLogPath() {
    try {
      await onCopyLogPath();
      logActionError = null;
    } catch {
      logActionError = t('copyLogFailed');
    }
  }
  async function revealLogFile() {
    try {
      await onOpenLogFolder();
      logActionError = null;
    } catch {
      logActionError = t('revealLogFailed');
    }
  }
  function record(event: KeyboardEvent) {
    if (!recording) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') {
      recording = false;
      return;
    }
    if (event.key === 'Delete' || event.key === 'Backspace') {
      patch({ globalShortcut: null });
      recording = false;
      return;
    }
    if (
      !(event.ctrlKey || event.altKey || event.metaKey) ||
      ['Control', 'Alt', 'Meta', 'Shift'].includes(event.key)
    )
      return;
    const modifiers = [
      event.ctrlKey && 'Ctrl',
      event.altKey && 'Alt',
      event.shiftKey && 'Shift',
      event.metaKey && 'Super',
    ].filter(Boolean);
    const key = event.code.startsWith('Key')
      ? event.code.slice(3)
      : event.code.startsWith('Digit')
        ? event.code.slice(5)
        : event.key.length === 1
          ? event.key.toUpperCase()
          : event.key;
    patch({ globalShortcut: [...modifiers, key].join('+') });
    recording = false;
  }
</script>

<section class="screen settings-screen" aria-label={t('settings')}>
  {#if settingsView.integrationError}<p class="notice" role="alert">
      {settingsView.integrationError}
    </p>{/if}

  {#if settingsView.platformSummary}<div class="settings-section">
      <h2>{t('linux')}</h2>
      <div class="setting-row">
        <span><b>{t('desktopIntegration')}</b><small>{settingsView.platformSummary}</small></span>
      </div>
    </div>{/if}

  <div class="settings-section">
    <h2>{t('general')}</h2>
    <label class="setting-row"
      ><span><b>{t('showTotalSpend')}</b></span><input
        type="checkbox"
        checked={settings.showTotalSpend}
        onchange={(event) => patch({ showTotalSpend: event.currentTarget.checked })}
      /></label
    >
    <label class="setting-row"
      ><span><b>{t('launchAtLogin')}</b></span><input
        type="checkbox"
        checked={settings.launchAtLogin}
        onchange={(event) => patch({ launchAtLogin: event.currentTarget.checked })}
      /></label
    >
    <div class="setting-row">
      <span><b>{t('globalShortcut')}</b></span>
      <div class="shortcut-field">
        <button
          class:recording
          type="button"
          data-tooltip={t('openFromAnywhere')}
          onclick={() => (recording = !recording)}
          onkeydown={record}
          >{recording
            ? t('typeShortcut')
            : (settings.globalShortcut ?? t('recordShortcut'))}</button
        >{#if settings.globalShortcut}<button
            class="clear-shortcut-button"
            type="button"
            aria-label={t('clearShortcut')}
            onclick={() => patch({ globalShortcut: null })}
            ><Icon name="close" size={10} strokeWidth={2.2} /></button
          >{/if}
      </div>
    </div>
  </div>

  <div class="settings-section">
    <h2>{t('appearance')}</h2>
    <div class="setting-row">
      <span><b>{t('language')}</b></span><SelectMenu
        label={t('language')}
        value={settings.language}
        options={[
          { value: 'system', label: t('systemLanguage') },
          { value: 'en', label: t('english') },
          { value: 'zh-CN', label: t('simplifiedChinese') },
          { value: 'zh-TW', label: t('traditionalChinese') },
        ]}
        onChange={(value) => patch({ language: value as AppSettings['language'] })}
      />
    </div>
    {#if platform === 'macos'}
      <div class="setting-row">
        <span><b>{t('iconStyle')}</b></span><SelectMenu
          label={t('iconStyle')}
          value={settings.menuBarStyle}
          options={[
            { value: 'text', label: t('text') },
            { value: 'bars', label: t('bars') },
          ]}
          onChange={(value) => patch({ menuBarStyle: value as AppSettings['menuBarStyle'] })}
        />
      </div>
    {/if}
    <div class="setting-row">
      <span><b>{t('theme')}</b></span><SelectMenu
        label={t('theme')}
        value={settings.theme}
        options={[
          { value: 'system', label: t('system') },
          { value: 'light', label: t('light') },
          { value: 'dark', label: t('dark') },
        ]}
        onChange={(value) => patch({ theme: value as AppSettings['theme'] })}
      />
    </div>
    <div class="setting-row">
      <span><b>{t('density')}</b></span><SelectMenu
        label={t('density')}
        value={settings.density}
        options={[
          { value: 'default', label: t('default') },
          { value: 'compact', label: t('compact') },
        ]}
        onChange={(value) => patch({ density: value as AppSettings['density'] })}
      />
    </div>
    <div class="setting-row">
      <span><b>{t('panelHeight')}</b></span><SelectMenu
        label={t('panelHeight')}
        value={panelHeightMode}
        options={[
          { value: 'automatic', label: t('automatic') },
          { value: 'manual', label: t('manual') },
        ]}
        onChange={(value) => onPanelHeightModeChange(value as PanelHeightMode)}
      />
    </div>
    <div class="setting-row">
      <span><b>{t('timeFormat')}</b></span><SelectMenu
        label={t('timeFormat')}
        value={settings.timeFormat}
        options={[
          { value: 'system', label: t('auto') },
          { value: 'twelveHour', label: t('twelveHour') },
          { value: 'twentyFourHour', label: t('twentyFourHour') },
        ]}
        onChange={(value) => patch({ timeFormat: value as AppSettings['timeFormat'] })}
      />
    </div>
  </div>

  <div class="settings-section">
    <h2>{t('usageDisplay')}</h2>
    <div class="setting-row">
      <span><b>{t('showUsageAs')}</b></span><SelectMenu
        label={t('showUsageAs')}
        value={settings.usageDisplay}
        options={[
          { value: 'left', label: t('left') },
          { value: 'used', label: t('used') },
        ]}
        onChange={(value) => patch({ usageDisplay: value as AppSettings['usageDisplay'] })}
      />
    </div>
    <div class="setting-row">
      <span><b>{t('resetTimes')}</b></span><SelectMenu
        label={t('resetTimes')}
        value={settings.resetDisplay}
        options={[
          { value: 'countdown', label: t('countdown') },
          { value: 'exact', label: t('exactTime') },
        ]}
        onChange={(value) => patch({ resetDisplay: value as AppSettings['resetDisplay'] })}
      />
    </div>
    <label class="setting-row"
      ><span
        ><b>{t('alwaysShowPacing')}</b><i
          class="setting-info"
          data-tooltip={t('pacingHelp')}
          aria-label={t('pacingHelp')}><Icon name="about" size={12} strokeWidth={1.8} /></i
        ></span
      ><input
        type="checkbox"
        checked={settings.alwaysShowPacing}
        onchange={(event) => patch({ alwaysShowPacing: event.currentTarget.checked })}
      /></label
    >
  </div>

  <div class="settings-section">
    <h2>
      {t('notifications')}
      {#if notificationsNeedAttention}<span class="permission-warning">!</span>{/if}
    </h2>
    <label class="setting-row"
      ><span
        ><b>{t('almostOut')}</b><i
          class="setting-info"
          data-tooltip={t('almostOutHelp')}
          aria-label={t('almostOutHelp')}><Icon name="about" size={12} strokeWidth={1.8} /></i
        ></span
      ><input
        type="checkbox"
        checked={settings.notifications.almostOut}
        onchange={(event) => patchNotification('almostOut', event.currentTarget.checked)}
      /></label
    >
    <label class="setting-row"
      ><span
        ><b>{t('cuttingItClose')}</b><i
          class="setting-info"
          data-tooltip={t('cuttingItCloseHelp')}
          aria-label={t('cuttingItCloseHelp')}><Icon name="about" size={12} strokeWidth={1.8} /></i
        ></span
      ><input
        type="checkbox"
        checked={settings.notifications.cuttingItClose}
        onchange={(event) => patchNotification('cuttingItClose', event.currentTarget.checked)}
      /></label
    >
    <label class="setting-row"
      ><span
        ><b>{t('willRunOut')}</b><i
          class="setting-info"
          data-tooltip={t('willRunOutHelp')}
          aria-label={t('willRunOutHelp')}><Icon name="about" size={12} strokeWidth={1.8} /></i
        ></span
      ><input
        type="checkbox"
        checked={settings.notifications.willRunOut}
        onchange={(event) => patchNotification('willRunOut', event.currentTarget.checked)}
      /></label
    >
    {#if notificationsNeedAttention}
      <div class="notification-actions">
        <div class="notification-attention" role="status">
          <span
            ><b
              >{settingsView.notificationPermission === 'denied'
                ? t('notificationsBlocked')
                : t('permissionRequired')}</b
            ><small
              >{settingsView.notificationPermission === 'denied'
                ? t('enableNotifications')
                : t('allowNotifications')}</small
            ></span
          >
          <button
            class="secondary-button"
            type="button"
            onclick={settingsView.notificationPermission === 'denied'
              ? onOpenNotificationSettings
              : onRequestNotifications}
            >{settingsView.notificationPermission === 'denied'
              ? t('openSettings')
              : t('allow')}</button
          >
        </div>
      </div>
    {/if}
  </div>

  <div class="settings-section">
    <h2>{t('advanced')}</h2>
    <div class="setting-row">
      <span><b>{t('logLevel')}</b></span><SelectMenu
        label={t('logLevel')}
        value={settings.logLevel}
        options={[
          { value: 'system', label: t('systemLanguage') },
          { value: 'error', label: t('error') },
          { value: 'warn', label: t('warning') },
          { value: 'info', label: t('info') },
          { value: 'debug', label: t('debug') },
        ]}
        onChange={(value) => patch({ logLevel: value as AppSettings['logLevel'] })}
      />
    </div>
    <div class="setting-row setting-row--button">
      <button class="secondary-button settings-wide-button" type="button" onclick={copyLogPath}
        >{t('copyLogPath')}</button
      >
    </div>
    <div class="setting-row setting-row--button">
      <button class="secondary-button settings-wide-button" type="button" onclick={revealLogFile}
        >{revealLogLabel}</button
      >
    </div>
    {#if logActionError}<p class="settings-note log-action-error" role="alert">
        {logActionError}
      </p>{/if}
  </div>

  <div class="settings-section">
    <h2>{t('updates')}</h2>
    <label class="setting-row"
      ><span><b>{t('autoUpdates')}</b></span><input
        type="checkbox"
        checked={settings.autoCheckUpdates}
        onchange={(event) => patch({ autoCheckUpdates: event.currentTarget.checked })}
      /></label
    >
    <div class="setting-row setting-row--button">
      <button
        type="button"
        class="secondary-button settings-wide-button"
        disabled={checkingUpdate}
        onclick={onCheckForUpdates}>{checkingUpdate ? t('checking') : t('checkUpdates')}</button
      >
    </div>
    {#if updateError}<div class="settings-update-error" role="alert">
        <b>{updateError.message}</b><small>{updateError.action}</small>
      </div>{/if}
  </div>

  <button class="screen-cross-link" type="button" aria-label={t('customize')} onclick={onCustomize}>
    <Icon name="sliders" size={17} />
    <span><b>{t('customize')}</b><small>{t('customizeDescription')}</small></span>
    <Icon name="chevron-right" size={13} strokeWidth={2.2} />
  </button>
</section>

<style>
  :global {
    .settings-section {
      margin-bottom: 10px;
    }

    .setting-row {
      display: flex;
      min-height: 40px;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
      padding: 6px 10px;
      border-top: 1px solid var(--separator);
      font-size: 11px;
    }

    .settings-section h2 + .setting-row {
      border-top: 0;
    }

    .setting-row > span {
      display: flex;
      min-width: 0;
      flex-direction: column;
      gap: 1px;
    }

    .setting-row b {
      font-weight: 550;
    }

    .setting-row small {
      color: var(--secondary);
      font-size: 9px;
      line-height: 12px;
    }

    input[type='checkbox'] {
      width: 15px;
      height: 15px;
      accent-color: var(--meter-fill);
    }

    .shortcut-field {
      display: flex;
      align-items: center;
      gap: 3px;
    }

    .shortcut-field button {
      max-width: 115px;
      padding: 4px 7px;
      overflow: hidden;
      border: 1px solid var(--separator);
      border-radius: 6px;
      color: var(--secondary);
      background: var(--tray);
      font-family: ui-monospace, monospace;
      font-size: 12px;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .shortcut-field button.recording {
      border-color: var(--meter-fill);
      color: var(--text);
    }

    .shortcut-field .clear-shortcut-button {
      display: grid;
      width: 24px;
      height: 24px;
      padding: 0;
      color: var(--secondary);
      font-family: inherit;
      place-items: center;
    }

    .shortcut-field .clear-shortcut-button:hover,
    .shortcut-field .clear-shortcut-button:focus-visible {
      outline: none;
      color: var(--text);
      background: var(--button-hover);
    }

    .secondary-button {
      flex: 0 0 auto;
      padding: 4px 8px;
      border: 1px solid var(--separator);
      border-radius: 6px;
      color: var(--text);
      background: var(--tray);
      font-size: 12px;
      font-weight: 500;
    }

    .secondary-button:disabled {
      opacity: 0.55;
    }

    .permission-warning {
      display: inline-grid;
      width: 13px;
      height: 13px;
      margin-left: 3px;
      border-radius: 50%;
      color: white;
      background: var(--warning);
      font-size: 8px;
      place-items: center;
    }

    .settings-note,
    .version-row {
      margin: 0;
      padding: 6px 10px 9px;
      color: var(--warning);
      font-size: 9px;
    }

    .version-row {
      padding: 3px 0 8px;
      color: var(--tertiary);
      text-align: center;
    }

    .settings-section {
      margin-bottom: 14px;
      overflow: visible;
      background: transparent;
    }

    .settings-section > .setting-row {
      border-top: 0;
      background: var(--card);
    }

    .settings-section > h2 + .setting-row {
      border-radius: 12px 12px 0 0;
    }

    .settings-section > .setting-row:last-child,
    .settings-section > .settings-note:last-child {
      border-radius: 0 0 12px 12px;
    }

    .settings-section > h2 + .setting-row:last-child {
      border-radius: 12px;
    }

    .setting-row {
      min-height: 40px;
      padding: 9px 12px;
      border: 0;
      font-size: 13px;
    }

    .setting-row b {
      font-weight: 400;
    }

    .setting-row .select-menu__trigger {
      font-size: 13px;
    }

    .setting-row small {
      font-size: 10px;
      line-height: 12px;
    }

    input[type='checkbox'] {
      width: 28px;
      height: 16px;
      flex: 0 0 auto;
      margin: 0;
      appearance: none;
      border-radius: 9px;
      background: var(--meter-track);
      cursor: pointer;
      transition: background-color 160ms ease;
    }

    input[type='checkbox']::after {
      display: block;
      width: 12px;
      height: 12px;
      margin: 2px;
      border-radius: 50%;
      background: white;
      box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
      content: '';
      transition: transform 160ms ease;
    }

    input[type='checkbox']:checked {
      background: var(--meter-fill);
    }

    input[type='checkbox']:checked::after {
      transform: translateX(12px);
    }

    .version-row {
      font-size: 10px;
    }

    .setting-row > span:has(.setting-info) {
      align-items: center;
      flex-direction: row;
      gap: 6px;
    }

    .setting-info {
      display: inline-grid;
      flex: 0 0 auto;
      color: var(--secondary);
      font-style: normal;
      place-items: center;
    }

    .setting-row--button {
      display: block;
    }

    .settings-wide-button {
      width: 100%;
      min-height: 28px;
      font-size: 12px;
    }

    .settings-note.log-action-error {
      background: var(--card);
    }

    .notification-actions {
      padding: 8px 12px 10px;
      border-top: 1px solid var(--separator);
      border-radius: 0 0 12px 12px;
      background: var(--card);
    }

    .notification-attention {
      display: flex;
      align-items: center;
      gap: 10px;
      color: var(--warning);
    }

    .notification-attention > span {
      display: flex;
      min-width: 0;
      flex: 1;
      flex-direction: column;
      gap: 2px;
    }

    .notification-attention b {
      font-size: 11px;
      font-weight: 600;
      line-height: 13px;
    }

    .notification-attention small {
      color: var(--secondary);
      font-size: 9px;
      line-height: 12px;
    }

    .notification-attention .secondary-button {
      flex: 0 0 auto;
    }

    .settings-update-error {
      display: flex;
      flex-direction: column;
      gap: 2px;
      margin: 0 12px 8px;
      padding: 8px;
      border-radius: 8px;
      color: var(--error);
      background: var(--error-bg);
    }

    .settings-update-error b {
      font-size: 11px;
      line-height: 14px;
    }

    .settings-update-error small {
      color: var(--error);
      font-size: 9px;
      line-height: 12px;
    }

    :root[data-density='compact'] .setting-row {
      gap: 8px;
      padding-right: 10px;
      padding-left: 10px;
    }

    :root[data-density='compact'] .screen-cross-link {
      min-height: 42px;
      margin-top: 8px;
    }
  }
</style>
