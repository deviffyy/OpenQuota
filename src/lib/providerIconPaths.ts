import antigravity from '../assets/provider-icons/antigravity.svg?raw';
import claude from '../assets/provider-icons/claude.svg?raw';
import codex from '../assets/provider-icons/codex.svg?raw';
import cursor from '../assets/provider-icons/cursor.svg?raw';
import grok from '../assets/provider-icons/grok.svg?raw';
import openrouter from '../assets/provider-icons/openrouter.svg?raw';
import zai from '../assets/provider-icons/zai.svg?raw';

const visuals: Record<string, { source: string; color: string | null }> = {
  antigravity: { source: antigravity, color: '#4285F4' },
  claude: { source: claude, color: '#DE7356' },
  codex: { source: codex, color: null },
  cursor: { source: cursor, color: null },
  grok: { source: grok, color: null },
  openrouter: { source: openrouter, color: null },
  zai: { source: zai, color: null },
};

export function providerIconPath(providerId: string) {
  const source = visuals[providerId]?.source;
  if (!source) return '';
  return [...source.matchAll(/<path\b[^>]*\bd="([^"]+)"/g)].map((match) => match[1]).join(' ');
}

export function providerIconColor(providerId: string) {
  return visuals[providerId]?.color ?? null;
}

export function providerIconViewBox(providerId: string) {
  return visuals[providerId]?.source.match(/viewBox="([^"]+)"/)?.[1] ?? '0 0 100 100';
}
