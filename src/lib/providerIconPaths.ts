import antigravity from '../assets/provider-icons/antigravity.svg?raw';
import claude from '../assets/provider-icons/claude.svg?raw';
import codex from '../assets/provider-icons/codex.svg?raw';

const visuals: Record<string, { source: string; color: string | null }> = {
  antigravity: { source: antigravity, color: '#4285F4' },
  claude: { source: claude, color: '#DE7356' },
  codex: { source: codex, color: null },
};

export function providerIconPath(providerId: string) {
  return visuals[providerId]?.source.match(/<path d="([^"]+)"/)?.[1] ?? '';
}

export function providerIconColor(providerId: string) {
  return visuals[providerId]?.color ?? null;
}
