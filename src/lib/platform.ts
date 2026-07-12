export type DesktopPlatform = 'macos' | 'windows' | 'linux';

export function desktopPlatform(
  userAgent = globalThis.navigator?.userAgent ?? '',
): DesktopPlatform {
  const normalized = userAgent.toLowerCase();
  if (normalized.includes('macintosh') || normalized.includes('mac os')) return 'macos';
  if (normalized.includes('windows')) return 'windows';
  return 'linux';
}

export function shortcutLabels(platform = desktopPlatform()) {
  return platform === 'macos'
    ? { settings: '⌘,', quit: '⌘Q' }
    : { settings: 'Ctrl+,', quit: 'Ctrl+Q' };
}
