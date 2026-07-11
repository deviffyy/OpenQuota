import antigravity from '../assets/provider-icons/antigravity.svg?raw';
import claude from '../assets/provider-icons/claude.svg?raw';
import codex from '../assets/provider-icons/codex.svg?raw';

const sources: Record<string, string> = { antigravity, claude, codex };

export function providerIconPath(providerId: string) {
  return (sources[providerId] ?? codex).match(/<path d="([^"]+)"/)?.[1] ?? '';
}
