import type { AppSettings } from './types';

export function restoreCustomization(current: AppSettings, previous: AppSettings): AppSettings {
  return {
    ...current,
    providers: previous.providers,
    detectionNoticeDismissed: previous.detectionNoticeDismissed,
  };
}
