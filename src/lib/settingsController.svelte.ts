import { getAppSettings, saveAppSettings } from './backend';
import type { AppSettings, SettingsViewState } from './types';

export class SettingsController {
  state = $state<SettingsViewState | null>(null);
  #saveQueue: Promise<void> = Promise.resolve();
  #revision = 0;
  #pendingSaves = 0;

  constructor(private readonly onError: (message: string) => void) {}

  setState(state: SettingsViewState) {
    this.state = state;
  }

  acceptExternalState(state: SettingsViewState) {
    if (this.#pendingSaves === 0) this.state = state;
  }

  async refreshIfIdle() {
    if (this.#pendingSaves !== 0) return;
    try {
      const state = await getAppSettings();
      if (this.#pendingSaves === 0) this.state = state;
    } catch {
      // Focus refresh is best-effort; the last known settings remain usable.
    }
  }

  save(next: AppSettings) {
    const current = this.state;
    if (!current) return;
    const revision = ++this.#revision;
    this.#pendingSaves += 1;
    this.state = { ...current, settings: next };
    this.#saveQueue = this.#saveQueue
      .then(async () => {
        const saved = await saveAppSettings(next);
        if (revision === this.#revision) this.state = saved;
      })
      .catch(async (error: unknown) => {
        if (revision !== this.#revision) return;
        this.onError(typeof error === 'string' ? error : 'Settings could not be saved.');
        try {
          this.state = await getAppSettings();
        } catch {
          this.onError('Settings could not be saved or reloaded.');
        }
      })
      .finally(() => {
        this.#pendingSaves -= 1;
      });
  }
}
