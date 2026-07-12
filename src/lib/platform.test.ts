import { describe, expect, it } from 'vitest';
import { desktopPlatform, shortcutLabels } from './platform';

describe('desktop platform presentation', () => {
  it('uses Command glyphs on macOS', () => {
    const platform = desktopPlatform('Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)');
    expect(platform).toBe('macos');
    expect(shortcutLabels(platform)).toEqual({ settings: '⌘,', quit: '⌘Q' });
  });

  it('uses Ctrl labels on Windows and Linux', () => {
    expect(desktopPlatform('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')).toBe('windows');
    expect(shortcutLabels('windows')).toEqual({ settings: 'Ctrl+,', quit: 'Ctrl+Q' });
    expect(shortcutLabels('linux')).toEqual({ settings: 'Ctrl+,', quit: 'Ctrl+Q' });
  });
});
