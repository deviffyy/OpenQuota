import { getAppSettings, saveAppSettings } from './backend';
import { t } from './i18n';
import type { AppSettings, SettingsViewState } from './types';

export class SettingsController {
  state = $state<SettingsViewState | null>(null);
  #saveQueue: Promise<void> = Promise.resolve();
  #revision = 0;
  #pendingSaves = 0;
  #externalRefreshPending = false;

  constructor(private readonly onError: (message: string) => void) {}

  setState(state: SettingsViewState) {
    if (!this.state || state.accountRevision >= this.state.accountRevision) this.state = state;
  }

  acceptExternalState(state: SettingsViewState) {
    if (this.#pendingSaves === 0) this.setState(state);
    else this.#externalRefreshPending = true;
  }

  async refreshIfIdle() {
    if (this.#pendingSaves !== 0) return;
    try {
      const state = await getAppSettings();
      if (this.#pendingSaves === 0) this.setState(state);
    } catch {
      // Focus refresh is best-effort; the last known settings remain usable.
    }
  }

  save(next: AppSettings) {
    const current = this.state;
    if (!current) return Promise.resolve();
    const revision = ++this.#revision;
    const expectedAccountRevision = current.accountRevision;
    this.#pendingSaves += 1;
    this.state = { ...current, settings: next };
    this.#saveQueue = this.#saveQueue
      .then(async () => {
        const saved = await saveAppSettings(next, expectedAccountRevision);
        if (revision === this.#revision) this.setState(saved);
      })
      .catch(async () => {
        if (revision !== this.#revision) return;
        this.onError(t('settingsSaveFailed'));
        try {
          this.setState(await getAppSettings());
        } catch {
          this.onError(t('settingsSaveReloadFailed'));
        }
      })
      .finally(() => {
        this.#pendingSaves -= 1;
        if (this.#pendingSaves === 0 && this.#externalRefreshPending) {
          this.#externalRefreshPending = false;
          void this.refreshIfIdle();
        }
      });
    return this.#saveQueue;
  }
}
