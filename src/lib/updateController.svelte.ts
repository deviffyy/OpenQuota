import { checkForApplicationUpdates, installApplicationUpdate, openUpdatePage } from './backend';
import { SvelteDate } from 'svelte/reactivity';
import type { UpdateFailure, UpdateProgress, UpdateStatus } from './types';

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
      if (manual) this.error = updateFailure(error, 'Updates could not be checked.');
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
      this.error = updateFailure(error, 'The update could not be installed.');
      this.installing = false;
      this.progress = null;
    }
  }

  async openDownloadPage() {
    try {
      await openUpdatePage();
    } catch (error) {
      this.error = updateFailure(error, 'The OpenQuota download page could not be opened.');
    }
  }

  setProgress(progress: UpdateProgress) {
    this.progress = progress;
  }
}

export function nextUpdateLabel(value: string | undefined, now: number) {
  if (!value) return 'Waiting for first update';
  const timestamp = Date.parse(value);
  if (Number.isNaN(timestamp)) return 'Next update unavailable';
  const seconds = Math.max(0, Math.ceil((timestamp + 300_000 - now) / 1000));
  return seconds >= 60
    ? `Next update in ${Math.ceil(seconds / 60)}m`
    : `Next update in ${seconds}s`;
}

export function updateFailure(error: unknown, fallback: string): UpdateFailure {
  if (error && typeof error === 'object') {
    const candidate = error as Partial<UpdateFailure>;
    if (typeof candidate.message === 'string') {
      return {
        code: typeof candidate.code === 'string' ? candidate.code : 'update_failed',
        message: candidate.message,
        action: typeof candidate.action === 'string' ? candidate.action : 'Try again later.',
        retryable: candidate.retryable !== false,
      };
    }
  }
  return {
    code: 'update_failed',
    message: typeof error === 'string' ? error : fallback,
    action: 'Try again or download the installer from the release page.',
    retryable: true,
  };
}

function updateCheckMessage(status: UpdateStatus) {
  if (!status.available) return `OpenQuota ${status.currentVersion} is up to date.`;
  return status.version
    ? `OpenQuota ${status.version} is available.`
    : 'An OpenQuota update is available.';
}
