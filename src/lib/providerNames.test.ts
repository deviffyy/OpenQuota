import { describe, expect, it } from 'vitest';
import { canRenameProvider, withProviderName } from './providerNames';
import type { AppSettings } from './types';

const settings = { providerNames: {} } as AppSettings;

describe('provider card names', () => {
  it('uses the account cards observed by the backend', () => {
    const observed = ['claude', 'claude@1234abcd', 'codex'];
    expect(canRenameProvider('claude', observed)).toBe(true);
    expect(canRenameProvider('claude@1234abcd', observed)).toBe(true);
    expect(canRenameProvider('codex', observed)).toBe(true);
    expect(canRenameProvider('grok', observed)).toBe(false);
  });

  it('stores a trimmed custom name and clears it with a blank value', () => {
    const renamed = withProviderName(settings, 'claude', '  Personal  ');
    expect(renamed.providerNames).toEqual({ claude: 'Personal' });
    expect(withProviderName(renamed, 'claude', 'Personal')).toBe(renamed);
    expect(withProviderName(renamed, 'claude', '   ').providerNames).toEqual({});
  });
});
