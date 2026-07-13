<script lang="ts">
  import Icon from './Icon.svelte';
  import SelectMenu from './SelectMenu.svelte';
  import type {
    AppSettings,
    NotificationPreferences,
    SettingsViewState,
    UpdateFailure,
  } from './types';

  interface Props {
    settingsView: SettingsViewState;
    onChange: (settings: AppSettings) => void;
    onRequestNotifications: () => void;
    onOpenNotificationSettings: () => void;
    updateError: UpdateFailure | null;
    checkingUpdate: boolean;
    onCheckForUpdates: () => void;
    onCustomize: () => void;
    onCopyDataPath: () => void;
  }
  let {
    settingsView,
    onChange,
    onRequestNotifications,
    onOpenNotificationSettings,
    updateError,
    checkingUpdate,
    onCheckForUpdates,
    onCustomize,
    onCopyDataPath,
  }: Props = $props();
  let recording = $state(false);
  const settings = $derived(settingsView.settings);
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

<section class="screen settings-screen" aria-label="Settings">
  {#if settingsView.integrationError}<p class="notice" role="alert">
      {settingsView.integrationError}
    </p>{/if}

  {#if settingsView.platformSummary}<div class="settings-section">
      <h2>Linux</h2>
      <div class="setting-row">
        <span><b>Desktop Integration</b><small>{settingsView.platformSummary}</small></span>
      </div>
    </div>{/if}

  <div class="settings-section">
    <h2>General</h2>
    <label class="setting-row"
      ><span><b>Show Total Spend</b></span><input
        type="checkbox"
        checked={settings.showTotalSpend}
        onchange={(event) => patch({ showTotalSpend: event.currentTarget.checked })}
      /></label
    >
    <label class="setting-row"
      ><span><b>Launch at Login</b></span><input
        type="checkbox"
        checked={settings.launchAtLogin}
        onchange={(event) => patch({ launchAtLogin: event.currentTarget.checked })}
      /></label
    >
    <div class="setting-row">
      <span><b>Global Shortcut</b></span>
      <div class="shortcut-field">
        <button
          class:recording
          type="button"
          data-tooltip="Open OpenQuota from anywhere"
          onclick={() => (recording = !recording)}
          onkeydown={record}
          >{recording ? 'Type Shortcut…' : (settings.globalShortcut ?? 'Record Shortcut')}</button
        >{#if settings.globalShortcut}<button
            type="button"
            aria-label="Clear global shortcut"
            onclick={() => patch({ globalShortcut: null })}
            ><Icon name="close" size={10} strokeWidth={2.2} /></button
          >{/if}
      </div>
    </div>
  </div>

  <div class="settings-section">
    <h2>Appearance</h2>
    <div class="setting-row">
      <span><b>Icon Style</b></span><SelectMenu
        label="Icon Style"
        value={settings.menuBarStyle}
        options={[
          { value: 'text', label: 'Text' },
          { value: 'bars', label: 'Bars' },
        ]}
        onChange={(value) => patch({ menuBarStyle: value as AppSettings['menuBarStyle'] })}
      />
    </div>
    <div class="setting-row">
      <span><b>Theme</b></span><SelectMenu
        label="Theme"
        value={settings.theme}
        options={[
          { value: 'system', label: 'System' },
          { value: 'light', label: 'Light' },
          { value: 'dark', label: 'Dark' },
        ]}
        onChange={(value) => patch({ theme: value as AppSettings['theme'] })}
      />
    </div>
    <div class="setting-row">
      <span><b>Density</b></span><SelectMenu
        label="Density"
        value={settings.density}
        options={[
          { value: 'default', label: 'Default' },
          { value: 'compact', label: 'Compact' },
        ]}
        onChange={(value) => patch({ density: value as AppSettings['density'] })}
      />
    </div>
    <div class="setting-row">
      <span><b>Time Format</b></span><SelectMenu
        label="Time Format"
        value={settings.timeFormat}
        options={[
          { value: 'system', label: 'Auto' },
          { value: 'twelveHour', label: '12-hour' },
          { value: 'twentyFourHour', label: '24-hour' },
        ]}
        onChange={(value) => patch({ timeFormat: value as AppSettings['timeFormat'] })}
      />
    </div>
  </div>

  <div class="settings-section">
    <h2>Usage Display</h2>
    <div class="setting-row">
      <span><b>Show Usage As</b></span><SelectMenu
        label="Show Usage As"
        value={settings.usageDisplay}
        options={[
          { value: 'left', label: 'Left' },
          { value: 'used', label: 'Used' },
        ]}
        onChange={(value) => patch({ usageDisplay: value as AppSettings['usageDisplay'] })}
      />
    </div>
    <div class="setting-row">
      <span><b>Reset Times</b></span><SelectMenu
        label="Reset Times"
        value={settings.resetDisplay}
        options={[
          { value: 'countdown', label: 'Countdown' },
          { value: 'exact', label: 'Exact Time' },
        ]}
        onChange={(value) => patch({ resetDisplay: value as AppSettings['resetDisplay'] })}
      />
    </div>
    <label class="setting-row"
      ><span
        ><b>Always Show Pacing</b><i
          class="setting-info"
          data-tooltip="Show how you're pacing on every metric, not just ones near their limit"
          aria-label="Show how you're pacing on every metric, not just ones near their limit"
          ><Icon name="about" size={12} strokeWidth={1.8} /></i
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
      Notifications {#if notificationsNeedAttention}<span class="permission-warning">!</span>{/if}
    </h2>
    <label class="setting-row"
      ><span
        ><b>Almost Out</b><i
          class="setting-info"
          data-tooltip="Alert when a limit drops below 10% remaining."
          aria-label="Alert when a limit drops below 10% remaining."
          ><Icon name="about" size={12} strokeWidth={1.8} /></i
        ></span
      ><input
        type="checkbox"
        checked={settings.notifications.almostOut}
        onchange={(event) => patchNotification('almostOut', event.currentTarget.checked)}
      /></label
    >
    <label class="setting-row"
      ><span
        ><b>Cutting It Close</b><i
          class="setting-info"
          data-tooltip="Alert when a limit is projected to finish with little left."
          aria-label="Alert when a limit is projected to finish with little left."
          ><Icon name="about" size={12} strokeWidth={1.8} /></i
        ></span
      ><input
        type="checkbox"
        checked={settings.notifications.cuttingItClose}
        onchange={(event) => patchNotification('cuttingItClose', event.currentTarget.checked)}
      /></label
    >
    <label class="setting-row"
      ><span
        ><b>Will Run Out</b><i
          class="setting-info"
          data-tooltip="Alert when a limit is projected to finish before it resets."
          aria-label="Alert when a limit is projected to finish before it resets."
          ><Icon name="about" size={12} strokeWidth={1.8} /></i
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
                ? 'Notifications are blocked'
                : 'Permission is required'}</b
            ><small
              >{settingsView.notificationPermission === 'denied'
                ? 'Enable OpenQuota notifications in system settings.'
                : 'Allow notifications to receive the alerts selected above.'}</small
            ></span
          >
          <button
            class="secondary-button"
            type="button"
            onclick={settingsView.notificationPermission === 'denied'
              ? onOpenNotificationSettings
              : onRequestNotifications}
            >{settingsView.notificationPermission === 'denied' ? 'Open Settings' : 'Allow'}</button
          >
        </div>
      </div>
    {/if}
  </div>

  <div class="settings-section">
    <h2>Advanced</h2>
    <div class="setting-row">
      <span><b>Application Data</b></span><button
        class="secondary-button"
        type="button"
        onclick={onCopyDataPath}>Copy Path</button
      >
    </div>
  </div>

  <div class="settings-section">
    <h2>Updates</h2>
    <label class="setting-row"
      ><span><b>Check for Updates Automatically</b></span><input
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
        onclick={onCheckForUpdates}>{checkingUpdate ? 'Checking…' : 'Check for Updates…'}</button
      >
    </div>
    {#if updateError}<div class="settings-update-error" role="alert">
        <b>{updateError.message}</b><small>{updateError.action}</small>
      </div>{/if}
  </div>

  <button class="screen-cross-link" type="button" aria-label="Customize" onclick={onCustomize}>
    <Icon name="sliders" size={17} />
    <span><b>Customize</b><small>Choose what's visible and where</small></span>
    <Icon name="chevron-right" size={13} strokeWidth={2.2} />
  </button>
</section>
