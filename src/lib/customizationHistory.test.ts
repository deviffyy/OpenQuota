import { describe, expect, it } from 'vitest';
import { settingsState } from '../test/appFixtures';
import { restoreCustomization } from './customizationHistory';

describe('customization history', () => {
  it('restores layout without reverting a later card rename', () => {
    const previous = structuredClone(settingsState.settings);
    const current = structuredClone(settingsState.settings);
    current.providerNames.codex = 'Work';
    current.providers[0].metrics.reverse();

    const restored = restoreCustomization(current, previous);

    expect(restored.providerNames).toEqual({ codex: 'Work' });
    expect(restored.providers).toEqual(previous.providers);
  });
});
