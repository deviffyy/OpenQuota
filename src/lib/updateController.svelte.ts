import { checkForApplicationUpdates, installApplicationUpdate, openUpdatePage } from './backend';
import { SvelteDate } from 'svelte/reactivity';
import { t, type TranslationKey } from './i18n';
import type { UpdateFailure, UpdateProgress, UpdateStatus } from './types';

const USAGE_REFRESH_INTERVAL_MS = 5 * 60_000;

const UPDATE_FAILURE_MESSAGE_KEYS: Record<string, TranslationKey> = {
  download_forbidden: 'updateDownloadForbidden',
  rate_limited: 'updateRateLimited',
  signature_invalid: 'updateSignatureInvalid',
  network: 'updateNetworkFailed',
  update_failed: 'updateOperationFailed',
  busy: 'updateBusy',
  not_configured: 'updateNotConfigured',
  manual_install_required: 'updateManualInstallRequired',
  up_to_date: 'updateAlreadyUpToDate',
};

const UPDATE_FAILURE_ACTION_KEYS: Record<string, TranslationKey> = {
  download_forbidden: 'tryAgainOrDownload',
  rate_limited: 'tryAgainLater',
  signature_invalid: 'tryAgainOrDownload',
  network: 'tryAgainLater',
  update_failed: 'tryAgainOrDownload',
  busy: 'tryAgainLater',
  not_configured: 'tryAgainOrDownload',
  manual_install_required: 'tryAgainOrDownload',
  up_to_date: 'noActionNeeded',
};

export class UpdateController {
  status = $state<UpdateStatus | null>(null);
  error = $state<UpdateFailure | null>(null);
  checking = $state(false);
  installing = $state(false);
  progress = $state<UpdateProgress | null>(null);

  async check(
    manual: boolean,
    onChecked: (checkedAt: string) => void,
    onMessage: (message: string) => void,
  ) {
    if (this.checking || this.installing) return;
    this.checking = true;
    if (manual) this.error = null;
    try {
      const status = await checkForApplicationUpdates();
      this.status = status;
      onChecked(new SvelteDate().toISOString());
      if (manual) onMessage(updateCheckMessage(status));
    } catch (error) {
      if (manual) this.error = updateFailure(error, t('updatesCheckFailed'));
    } finally {
      this.checking = false;
    }
  }

  async install() {
    if (this.installing || this.checking) return;
    this.installing = true;
    this.progress = { phase: 'downloading', downloaded: 0, total: null, percent: null };
    this.error = null;
    try {
      await installApplicationUpdate();
    } catch (error) {
      this.error = updateFailure(error, t('updateInstallFailed'));
      this.installing = false;
      this.progress = null;
    }
  }

  async openDownloadPage() {
    try {
      await openUpdatePage();
    } catch (error) {
      this.error = updateFailure(error, t('updatePageOpenFailed'));
    }
  }

  setProgress(progress: UpdateProgress) {
    this.progress = progress;
  }
}

export function nextUpdateLabel(value: string | undefined, now: number) {
  if (!value) return t('waitingForFirstUpdate');
  const timestamp = Date.parse(value);
  if (Number.isNaN(timestamp)) return t('nextUpdateUnavailable');
  const remaining = Math.min(
    USAGE_REFRESH_INTERVAL_MS,
    Math.max(0, timestamp + USAGE_REFRESH_INTERVAL_MS - now),
  );
  const seconds = Math.ceil(remaining / 1000);
  return seconds >= 60
    ? t('nextUpdateMinutes', { count: Math.ceil(seconds / 60) })
    : t('nextUpdateSeconds', { count: seconds });
}

export function updateFailure(error: unknown, fallback: string): UpdateFailure {
  if (error && typeof error === 'object') {
    const candidate = error as Partial<UpdateFailure>;
    const code = typeof candidate.code === 'string' ? candidate.code : 'update_failed';
    const messageKey = UPDATE_FAILURE_MESSAGE_KEYS[code];
    const actionKey = UPDATE_FAILURE_ACTION_KEYS[code];
    if (messageKey) {
      return {
        code,
        message: t(messageKey),
        action: t(actionKey ?? 'tryAgainLater'),
        retryable: candidate.retryable !== false,
      };
    }
    if (typeof candidate.message === 'string') {
      return {
        code,
        message: candidate.message,
        action: typeof candidate.action === 'string' ? candidate.action : t('tryAgainLater'),
        retryable: candidate.retryable !== false,
      };
    }
  }
  return {
    code: 'update_failed',
    message: typeof error === 'string' ? error : fallback,
    action: t('tryAgainOrDownload'),
    retryable: true,
  };
}

function updateCheckMessage(status: UpdateStatus) {
  if (!status.available) return t('versionUpToDate', { version: status.currentVersion });
  return status.version
    ? t('versionAvailable', { version: status.version })
    : t('updateAvailableGeneric');
}
