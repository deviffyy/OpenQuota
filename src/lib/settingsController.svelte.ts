import { getAppSettings, saveAppSettings } from './backend';
import { t } from './i18n';
import type { AppSettings, SettingsViewState } from './types';

export type SettingsMutation = (
  expectedSettingsRevision: number,
  expectedAccountRevision: number,
) => Promise<SettingsViewState>;

class CancelledSettingsMutation extends Error {
  constructor() {
    super(t('settingsMutationCancelled'));
  }
}

function doesNotPrecede(candidate: SettingsViewState, current: SettingsViewState) {
  return (
    candidate.settingsRevision >= current.settingsRevision &&
    candidate.accountRevision >= current.accountRevision
  );
}

function strictlyFollows(candidate: SettingsViewState, current: SettingsViewState) {
  return (
    doesNotPrecede(candidate, current) &&
    (candidate.settingsRevision > current.settingsRevision ||
      candidate.accountRevision > current.accountRevision)
  );
}

export class SettingsController {
  state = $state<SettingsViewState | null>(null);
  #mutationQueue: Promise<void> = Promise.resolve();
  #mutationSequence = 0;
  #mutationGeneration = 0;
  #pendingMutations = 0;
  #externalRefreshPending = false;
  #pendingExternalState: SettingsViewState | null = null;
  #draftExternalState: SettingsViewState | null = null;
  #draftActive = false;
  #serverState: SettingsViewState | null = null;

  constructor(private readonly onError: (message: string) => void) {}

  setState(state: SettingsViewState) {
    if (this.state && !doesNotPrecede(state, this.state)) return;
    this.state = state;
    if (this.#pendingMutations === 0) this.#recordServerState(state);
  }

  acceptExternalState(state: SettingsViewState) {
    if (this.#draftActive) {
      if (!this.#draftExternalState || doesNotPrecede(state, this.#draftExternalState)) {
        this.#draftExternalState = state;
      }
      return;
    }
    if (this.#pendingMutations === 0) this.setState(state);
    else {
      if (!this.#pendingExternalState || doesNotPrecede(state, this.#pendingExternalState)) {
        this.#pendingExternalState = state;
      }
      this.#externalRefreshPending = true;
    }
  }

  async refreshIfIdle() {
    if (this.#pendingMutations !== 0 || this.#draftActive) return;
    const sequence = this.#mutationSequence;
    try {
      const state = await getAppSettings();
      if (this.#pendingMutations === 0 && sequence === this.#mutationSequence) {
        this.setState(state);
      } else {
        this.#externalRefreshPending = true;
      }
    } catch {
      // Focus refresh is best-effort; the last known settings remain usable.
    }
  }

  save(next: AppSettings) {
    const current = this.state;
    if (!current) return Promise.resolve();
    this.state = { ...current, settings: next };
    return this.#enqueueMutation(
      (expectedSettingsRevision, expectedAccountRevision) =>
        saveAppSettings(next, expectedSettingsRevision, expectedAccountRevision),
      true,
    ).catch(() => undefined);
  }

  beginDraft() {
    this.#draftActive = true;
  }

  setDraftSettings(next: AppSettings) {
    if (!this.state) return;
    this.state = { ...this.state, settings: next };
  }

  endDraft() {
    if (!this.#draftActive) return;
    this.#draftActive = false;
    const external = this.#draftExternalState;
    this.#draftExternalState = null;
    if (external) this.acceptExternalState(external);
  }

  runMutation(mutation: SettingsMutation) {
    if (!this.state) return Promise.reject(t('settingsUnavailable'));
    return this.#enqueueMutation(mutation);
  }

  #enqueueMutation(mutation: SettingsMutation, reportError = false) {
    const sequence = ++this.#mutationSequence;
    const generation = this.#mutationGeneration;
    this.#pendingMutations += 1;

    const task = this.#mutationQueue.then(async () => {
      if (generation !== this.#mutationGeneration) throw new CancelledSettingsMutation();
      const base = this.#serverState ?? this.state;
      if (!base) throw new Error(t('settingsUnavailable'));

      try {
        const saved = await mutation(base.settingsRevision, base.accountRevision);
        if (this.#recordServerState(saved) && sequence === this.#mutationSequence) {
          if (!this.state || doesNotPrecede(saved, this.state)) this.state = saved;
        }
      } catch (error) {
        if (error instanceof CancelledSettingsMutation) throw error;
        if (generation === this.#mutationGeneration) {
          if (reportError) this.onError(t('settingsSaveFailed'));
          const latestAtFailure = this.#mutationSequence;
          this.#mutationGeneration += 1;
          await this.#reloadAfterFailure(latestAtFailure);
        }
        throw error;
      }
    });

    const completed = task.finally(() => {
      this.#pendingMutations -= 1;
      if (this.#pendingMutations === 0 && this.#externalRefreshPending) {
        this.#externalRefreshPending = false;
        const pendingExternalState = this.#pendingExternalState;
        this.#pendingExternalState = null;
        if (
          pendingExternalState &&
          (!this.#serverState || strictlyFollows(pendingExternalState, this.#serverState))
        ) {
          this.setState(pendingExternalState);
        }
        void this.refreshIfIdle();
      }
    });
    this.#mutationQueue = completed.catch(() => undefined);
    return completed;
  }

  #recordServerState(state: SettingsViewState) {
    if (this.#serverState && !doesNotPrecede(state, this.#serverState)) return false;
    this.#serverState = state;
    return true;
  }

  async #reloadAfterFailure(latestAtFailure: number) {
    try {
      const state = await getAppSettings();
      if (!this.#recordServerState(state)) return;
      if (
        latestAtFailure === this.#mutationSequence &&
        (!this.state || doesNotPrecede(state, this.state))
      ) {
        this.state = state;
      }
    } catch {
      this.onError(t('settingsSaveReloadFailed'));
    }
  }
}
