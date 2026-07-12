import { describe, expect, it } from 'vitest';
import css from '../app.css?raw';
import tauriConfigSource from '../../src-tauri/tauri.conf.json?raw';
import { PANEL_MIN_HEIGHT, PANEL_SCREEN_FRACTION } from './panelSizing';

const tauriConfig = JSON.parse(tauriConfigSource) as {
  app: { windows: Array<{ width: number; minHeight: number }> };
};

describe('popover geometry contract', () => {
  it('keeps the reference popover and display bounds', () => {
    expect(tauriConfig.app.windows[0]).toMatchObject({ width: 320, minHeight: 200 });
    expect(PANEL_MIN_HEIGHT).toBe(200);
    expect(PANEL_SCREEN_FRACTION).toBe(0.85);
  });

  it('keeps the reference regular-density spacing and chrome dimensions', () => {
    expect(css).toMatch(/\.content\s*{[^}]*padding: 14px 14px 12px;/s);
    expect(css).toMatch(/\.content\s*{[^}]*overflow-y: auto;[^}]*scrollbar-width: none;/s);
    expect(css).toMatch(/\.content::-webkit-scrollbar\s*{[^}]*width: 0;[^}]*height: 0;/s);
    expect(css).toMatch(/\.provider-card\s*{[^}]*border-radius: 12px;/s);
    expect(css).not.toContain('.provider-card--pending');
    expect(css).toMatch(
      /\.provider-warning::after,[\s\S]*right: 0;[\s\S]*left: auto;[\s\S]*transform-origin: top right;/,
    );
    expect(css).toMatch(
      /\.total-card__info::after,[\s\S]*right: auto;[\s\S]*left: 0;[\s\S]*transform-origin: top left;/,
    );
    expect(css).toContain('max-width: min(190px, calc(100vw - 24px))');
    expect(css).toMatch(/\.metric\s*{[^}]*padding: 10px 14px;/s);
    expect(css).toMatch(/\.meter\s*{[^}]*height: 5px;/s);
    expect(css).toMatch(/\.app-top-bar\s*{[^}]*min-height: 44px;/s);
    expect(css).toMatch(/\.footer\s*{[^}]*min-height: 52px;/s);
  });
});
