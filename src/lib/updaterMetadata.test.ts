import { describe, expect, it } from 'vitest';
import { normalizeUpdaterMetadata } from '../../.github/scripts/normalize-updater-json.mjs';

describe('release updater metadata', () => {
  const release = {
    assets: [
      { id: 42, name: 'OpenQuota_0.2.0_x64-setup.exe' },
      { id: 43, name: 'OpenQuota_0.2.0_x64-setup.exe.sig' },
    ],
  };

  it('replaces GitHub API asset URLs with public browser download URLs', () => {
    const update = {
      platforms: {
        'windows-x86_64': {
          url: 'https://api.github.com/repos/deviffyy/OpenQuota/releases/assets/42',
          signature: 'signed',
        },
      },
    };

    expect(
      normalizeUpdaterMetadata(update, release, 'deviffyy/OpenQuota', 'v0.2.0').platforms[
        'windows-x86_64'
      ].url,
    ).toBe(
      'https://github.com/deviffyy/OpenQuota/releases/download/v0.2.0/OpenQuota_0.2.0_x64-setup.exe',
    );
  });

  it('rejects metadata whose artifact or signature is missing', () => {
    expect(() =>
      normalizeUpdaterMetadata(
        {
          platforms: {
            'windows-x86_64': {
              url: 'https://api.github.com/repos/deviffyy/OpenQuota/releases/assets/99',
              signature: 'signed',
            },
          },
        },
        release,
        'deviffyy/OpenQuota',
        'v0.2.0',
      ),
    ).toThrow('Unknown updater asset');
  });
});
