<script lang="ts">
  import Icon from './Icon.svelte';
  import type {
    AppSettings,
    NotificationPreferences,
    SettingsViewState,
    UpdateStatus,
  } from './types';

  interface Props {
    settingsView: SettingsViewState;
    onChange: (settings: AppSettings) => void;
    onRequestNotifications: () => void;
    updateStatus: UpdateStatus | null;
    updateError: string | null;
    checkingUpdate: boolean;
    installingUpdate: boolean;
    onCheckForUpdates: () => void;
    onInstallUpdate: () => void;
    onCustomize: () => void;
    onCopyDataPath: () => void;
  }
  let {
    settingsView,
    onChange,
    onRequestNotifications,
    updateStatus,
    updateError,
    checkingUpdate,
    installingUpdate,
    onCheckForUpdates,
    onInstallUpdate,
    onCustomize,
    onCopyDataPath,
  }: Props = $props();
  let recording = $state(false);
  const settings = $derived(settingsView.settings);

  function patch(value: Partial<AppSettings>) {
    onChange({ ...settings, ...value });
  }
  function patchNotification(key: keyof NotificationPreferences, enabled: boolean) {
    if (enabled && settingsView.notificationPermission === 'prompt') onRequestNotifications();
    patch({ notifications: { ...settings.notifications, [key]: enabled } });
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
    <h2>Startup</h2>
    <label class="setting-row"
      ><span><b>Show Total Spend</b><small>Combined cost and token summary.</small></span><input
        type="checkbox"
        checked={settings.showTotalSpend}
        onchange={(event) => patch({ showTotalSpend: event.currentTarget.checked })}
      /></label
    >
    <label class="setting-row"
      ><span><b>Launch at Login</b><small>Start OpenQuota with your computer.</small></span><input
        type="checkbox"
        checked={settings.launchAtLogin}
        onchange={(event) => patch({ launchAtLogin: event.currentTarget.checked })}
      /></label
    >
    <div class="setting-row">
      <span><b>Global Shortcut</b><small>Toggle the popup from anywhere.</small></span>
      <div class="shortcut-field">
        <button
          class:recording
          type="button"
          onclick={() => (recording = !recording)}
          onkeydown={record}
          >{recording ? 'Type Shortcut…' : (settings.globalShortcut ?? 'Record Shortcut')}</button
        >{#if settings.globalShortcut}<button
            type="button"
            aria-label="Clear global shortcut"
            onclick={() => patch({ globalShortcut: null })}>×</button
          >{/if}
      </div>
    </div>
  </div>

  <div class="settings-section">
    <h2>Appearance</h2>
    <label class="setting-row"
      ><span><b>Menu Bar</b><small>Show pinned values as text or compact usage bars.</small></span
      ><select
        value={settings.menuBarStyle}
        onchange={(event) =>
          patch({ menuBarStyle: event.currentTarget.value as AppSettings['menuBarStyle'] })}
        ><option value="text">Text</option><option value="bars">Bars</option></select
      ></label
    >
    <label class="setting-row"
      ><span><b>Theme</b></span><select
        value={settings.theme}
        onchange={(event) => patch({ theme: event.currentTarget.value as AppSettings['theme'] })}
        ><option value="system">System</option><option value="light">Light</option><option
          value="dark">Dark</option
        ></select
      ></label
    >
    <label class="setting-row"
      ><span><b>Density</b></span><select
        value={settings.density}
        onchange={(event) =>
          patch({ density: event.currentTarget.value as AppSettings['density'] })}
        ><option value="default">Default</option><option value="compact">Compact</option></select
      ></label
    >
    <label class="setting-row"
      ><span><b>Time Format</b></span><select
        aria-label="Time Format"
        value={settings.timeFormat}
        onchange={(event) =>
          patch({ timeFormat: event.currentTarget.value as AppSettings['timeFormat'] })}
        ><option value="system">Automatic</option><option value="twelveHour">12 Hour</option><option
          value="twentyFourHour">24 Hour</option
        ></select
      ></label
    >
  </div>

  <div class="settings-section">
    <h2>Usage Display</h2>
    <label class="setting-row"
      ><span><b>Show Usage As</b></span><select
        value={settings.usageDisplay}
        onchange={(event) =>
          patch({ usageDisplay: event.currentTarget.value as AppSettings['usageDisplay'] })}
        ><option value="left">Left</option><option value="used">Used</option></select
      ></label
    >
    <label class="setting-row"
      ><span><b>Reset Times</b></span><select
        value={settings.resetDisplay}
        onchange={(event) =>
          patch({ resetDisplay: event.currentTarget.value as AppSettings['resetDisplay'] })}
        ><option value="countdown">Countdown</option><option value="exact">Exact Time</option
        ></select
      ></label
    >
    <label class="setting-row"
      ><span
        ><b>Always Show Pacing</b><small>Show projections even when usage is healthy.</small></span
      ><input
        type="checkbox"
        checked={settings.alwaysShowPacing}
        onchange={(event) => patch({ alwaysShowPacing: event.currentTarget.checked })}
      /></label
    >
  </div>

  <div class="settings-section">
    <h2>Updates</h2>
    <label class="setting-row"
      ><span
        ><b>Automatic Checks</b><small>Look for signed updates when OpenQuota starts.</small></span
      ><input
        type="checkbox"
        checked={settings.autoCheckUpdates}
        onchange={(event) => patch({ autoCheckUpdates: event.currentTarget.checked })}
      /></label
    >
    <div class="setting-row">
      <span
        ><b
          >{updateStatus?.available
            ? `OpenQuota ${updateStatus.version} is available`
            : updateStatus
              ? 'OpenQuota is up to date'
              : 'Check for Updates'}</b
        ><small
          >{updateError ??
            updateStatus?.body ??
            `Current version ${updateStatus?.currentVersion ?? '0.1.0'}`}</small
        ></span
      >
      {#if updateStatus?.available}<button
          type="button"
          class="secondary-button"
          disabled={installingUpdate}
          onclick={onInstallUpdate}>{installingUpdate ? 'Installing…' : 'Install'}</button
        >{:else}<button
          type="button"
          class="secondary-button"
          disabled={checkingUpdate}
          onclick={onCheckForUpdates}>{checkingUpdate ? 'Checking…' : 'Check'}</button
        >{/if}
    </div>
  </div>

  <div class="settings-section">
    <h2>
      Notifications {#if settingsView.notificationPermission === 'denied'}<span
          class="permission-warning">!</span
        >{/if}
    </h2>
    <label class="setting-row"
      ><span><b>Almost Out</b><small>Under 10% remaining.</small></span><input
        type="checkbox"
        checked={settings.notifications.almostOut}
        onchange={(event) => patchNotification('almostOut', event.currentTarget.checked)}
      /></label
    >
    <label class="setting-row"
      ><span><b>Cutting It Close</b><small>Projected to finish near the limit.</small></span><input
        type="checkbox"
        checked={settings.notifications.cuttingItClose}
        onchange={(event) => patchNotification('cuttingItClose', event.currentTarget.checked)}
      /></label
    >
    <label class="setting-row"
      ><span><b>Will Run Out</b><small>Projected to run out before reset.</small></span><input
        type="checkbox"
        checked={settings.notifications.willRunOut}
        onchange={(event) => patchNotification('willRunOut', event.currentTarget.checked)}
      /></label
    >
    {#if settingsView.notificationPermission === 'denied'}<p class="settings-note">
        Notifications are blocked in system settings.
      </p>{/if}
  </div>

  <div class="settings-section">
    <h2>Privacy</h2>
    <div class="setting-row">
      <span
        ><b>Anonymous Usage</b><small
          >OpenQuota sends no analytics; settings and snapshots stay local.</small
        ></span
      ><strong class="setting-status">Off</strong>
    </div>
  </div>

  <div class="settings-section">
    <h2>Advanced</h2>
    <div class="setting-row">
      <span><b>Application Data</b><small>SQLite cache, settings and local snapshots.</small></span
      ><button class="secondary-button" type="button" onclick={onCopyDataPath}>Copy Path</button>
    </div>
  </div>

  <button class="screen-cross-link" type="button" aria-label="Customize" onclick={onCustomize}>
    <Icon name="sliders" size={17} />
    <span><b>Customize</b><small>Choose what's visible and where</small></span>
    <Icon name="chevron-right" size={13} strokeWidth={2.2} />
  </button>
</section>
