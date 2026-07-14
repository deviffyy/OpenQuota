import { describe, expect, it } from 'vitest';
import baseCss from '../styles/base.css?raw';
import layoutCss from '../styles/layout.css?raw';
import componentCss from '../styles/components.css?raw';
import tokensCss from '../styles/tokens.css?raw';
import tauriConfigSource from '../../src-tauri/tauri.conf.json?raw';
import { PANEL_MIN_HEIGHT, PANEL_SCREEN_FRACTION } from './panelSizing';
import { coLocatedComponentCss } from './uiStyleSources';

const css = `${layoutCss}\n${componentCss}\n${coLocatedComponentCss}`;

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

  it('keeps shared rules below component-owned styles regardless of bundle order', () => {
    expect(tokensCss).toContain('@layer tokens, base, shared;');
    expect(baseCss).toContain('@layer base');
    expect(layoutCss).toContain('@layer shared');
    expect(componentCss).toContain('@layer shared');
    expect(coLocatedComponentCss).not.toContain('@layer shared');
  });

  it('keeps reorder handles reachable on touch and hybrid-pointer devices', () => {
    expect(css).toContain('@media (hover: none), (pointer: coarse), (any-pointer: coarse)');
    expect(css).toMatch(/\.metric-reorder-handle\s*{[^}]*width: 44px;[^}]*height: 44px;/s);
    expect(css).toMatch(
      /\.drag-grip::after,[\s\S]*\.reorder-grip::after\s*{[^}]*width: 44px;[^}]*height: 44px;/,
    );
  });

  it('keeps compact rows aligned and compact controls genuinely dense', () => {
    expect(css).toMatch(/:root\[data-density='compact'\] \.usage-row\s*{[^}]*padding: 3px 14px;/s);
    expect(css).toMatch(/:root\[data-density='compact'\] \.trend-row\s*{[^}]*padding: 6px 14px;/s);
    expect(css).toMatch(
      /:root\[data-density='compact'\] \.select-menu__trigger\s*{[^}]*min-height: 26px;/s,
    );
    expect(css).toMatch(
      /:root\[data-density='compact'\] \.screen-cross-link\s*{[^}]*min-height: 42px;/s,
    );
    expect(css).toMatch(
      /:root\[data-density='compact'\] \.spend-ring\s*{[^}]*width: 88px;[^}]*height: 88px;/s,
    );
  });

  it('keeps drag depth on painted surfaces instead of darkening transparent headers', () => {
    const layerRule = css.match(/\.pointer-reorder-layer\s*{([^}]*)}/s)?.[1] ?? '';
    const liftRule = css.match(/\.pointer-reorder-lift\s*{([^}]*)}/s)?.[1] ?? '';
    expect(layerRule).toContain('position: fixed');
    expect(layerRule).toContain('inset: 0');
    expect(layerRule).toContain('isolation: isolate');
    expect(liftRule).toContain('position: absolute');
    expect(liftRule).toContain('color: var(--text)');
    expect(liftRule).toContain('opacity: 1');
    expect(liftRule).not.toContain('box-shadow');
    expect(css).toMatch(
      /\.pointer-reorder-lift\.provider-section > \.provider-card,[\s\S]*box-shadow: 0 8px 28px rgba\(0, 0, 0, 0\.18\);/,
    );
  });
});
