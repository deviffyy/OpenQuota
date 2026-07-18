import { afterEach, describe, expect, it, vi } from 'vitest';
import { codexState, providerCatalogIndex, settingsState } from '../test/appFixtures';
import { ProviderCatalogIndex } from './metrics';
import totalSpendSource from './TotalSpend.svelte?raw';
import {
  buildProviderShareRows as buildProviderShareRowsWithCatalog,
  providerIconPlacement,
  providerShareCardHeight,
  renderTotalSpendShareCard as renderTotalSpendShareCardWithCatalog,
  SHARE_CARD_SCALE,
  SHARE_CARD_WIDTH,
  TOTAL_SPEND_GEOMETRY,
  TOTAL_SPEND_OUTER_PADDING,
  TOTAL_SPEND_PERIOD_LABELS,
  totalSpendShareCardHeight,
} from './shareCard';
import type { AppSettings, ProviderLayout, ProviderSnapshot } from './types';

function buildProviderShareRows(
  _providerId: string,
  snapshot: ProviderSnapshot,
  layout: ProviderLayout,
  settings: AppSettings,
  now: number,
) {
  return buildProviderShareRowsWithCatalog(providerCatalogIndex, snapshot, layout, settings, now);
}

function renderTotalSpendShareCard(
  options: Parameters<typeof renderTotalSpendShareCardWithCatalog>[1],
) {
  return renderTotalSpendShareCardWithCatalog(providerCatalogIndex, options);
}

afterEach(() => vi.restoreAllMocks());

describe('share card layout', () => {
  it('uses the authored width and 4x export scale', () => {
    expect(SHARE_CARD_WIDTH).toBe(360);
    expect(SHARE_CARD_SCALE).toBe(4);
  });

  it('aspect-fits non-100 provider viewBoxes in exported headers', () => {
    expect(providerIconPlacement('codex', 16, 16, 22)).toEqual({
      x: 16,
      y: 16,
      scale: 0.22,
    });
    expect(providerIconPlacement('devin', 16, 16, 22)).toEqual({
      x: 17.32,
      y: 16,
      scale: 0.44,
    });
    const opencode = providerIconPlacement('opencode', 16, 16, 22);
    expect(opencode.x).toBeCloseTo(18.2);
    expect(opencode.y).toBe(16);
    expect(opencode.scale).toBeCloseTo(22 / 30);
  });

  it('exports only the provider rows visible on the dashboard', () => {
    const snapshot = codexState.snapshot!;
    const settings = settingsState.settings;
    const collapsed = settings.providers[0];
    const collapsedRows = buildProviderShareRows(
      'codex',
      snapshot,
      collapsed,
      settings,
      Date.now(),
    );

    expect(collapsedRows.map((row) => row.kind)).toEqual(['quota', 'quota', 'trend']);

    const expandedRows = buildProviderShareRows(
      'codex',
      snapshot,
      { ...collapsed, expanded: true },
      settings,
      Date.now(),
    );
    expect(expandedRows.map((row) => row.kind)).toEqual([
      'quota',
      'quota',
      'trend',
      'quota',
      'quota',
      'text',
      'text',
      'text',
      'text',
      'text',
    ]);
    expect(expandedRows.slice(-3)).toMatchObject([
      { condensed: true },
      { condensed: true },
      { condensed: true },
    ]);
  });

  it('does not encode unknown pricing as an approximation prefix', () => {
    const snapshot = structuredClone(codexState.snapshot!);
    snapshot.usage.today = {
      tokens: 500,
      estimatedCostUsd: 0.03,
      costEstimated: true,
      estimateComplete: false,
      unknownModels: ['future-unpriced-model'],
    };
    const rows = buildProviderShareRows(
      'codex',
      snapshot,
      { ...settingsState.settings.providers[0], expanded: true },
      settingsState.settings,
      Date.now(),
    );

    expect(rows.find((row) => row.kind === 'text' && row.label === 'Today')).toMatchObject({
      value: '$0.03 · 500 tokens',
    });
  });

  it('keeps provider notices in exported cards', () => {
    const snapshot = structuredClone(codexState.snapshot!);
    snapshot.notices = [
      {
        id: 'rateLimited',
        title: 'Live usage paused',
        message: 'Retrying in about 5 minutes',
        tone: 'warning',
      },
    ];
    const rows = buildProviderShareRows(
      'codex',
      snapshot,
      settingsState.settings.providers[0],
      settingsState.settings,
      Date.now(),
    );

    expect(rows[0]).toMatchObject({
      kind: 'text',
      label: 'Live usage paused',
      value: 'Retrying in about 5 minutes',
    });
  });

  it('keeps provider-supplied count units in exported quota rows', () => {
    const snapshot = structuredClone(codexState.snapshot!);
    snapshot.quotas[0] = {
      ...snapshot.quotas[0],
      format: 'count',
      usedPercent: 25,
      usedValue: 25,
      limitValue: 100,
      unit: 'searches',
    };

    const rows = buildProviderShareRows(
      'codex',
      snapshot,
      settingsState.settings.providers[0],
      settingsState.settings,
      Date.now(),
    );

    expect(rows[0]).toMatchObject({ kind: 'quota', reading: '75 searches left' });
  });

  it('exports customizable status metrics as text rows', () => {
    const catalog = new ProviderCatalogIndex({
      providers: [
        {
          id: 'grok',
          displayName: 'Grok',
          shortName: 'G',
          fallbackEnabled: false,
          localUsageSourceNote: null,
          links: [],
          metrics: [
            {
              id: 'grok.payAsYouGo',
              label: 'Extra Usage',
              source: { kind: 'status', sourceId: 'payAsYouGo' },
              pinnable: true,
              defaultEnabled: true,
              defaultSection: 'alwaysVisible',
              defaultPinned: true,
              tray: { shortLabel: 'E', suffix: null },
            },
          ],
        },
      ],
    });
    const snapshot: ProviderSnapshot = {
      providerId: 'grok',
      plan: null,
      quotas: [],
      valueMetrics: [],
      statusMetrics: [
        {
          id: 'payAsYouGo',
          label: 'Extra Usage',
          text: '2500 cap',
          tone: 'positive',
        },
      ],
      notices: [],
      usage: { today: null, yesterday: null, last30Days: null, daily: [], unknownModels: [] },
      warnings: [],
      refreshedAt: '2026-07-18T00:00:00Z',
    };
    const layout: ProviderLayout = {
      id: 'grok',
      enabled: true,
      detected: true,
      expanded: false,
      metrics: [
        {
          id: 'grok.payAsYouGo',
          enabled: true,
          section: 'alwaysVisible',
          pinned: true,
        },
      ],
    };

    expect(
      buildProviderShareRowsWithCatalog(
        catalog,
        snapshot,
        layout,
        settingsState.settings,
        Date.now(),
      ),
    ).toEqual([{ kind: 'text', label: 'Extra Usage', value: '2500 cap', condensed: false }]);
  });

  it('keeps always-visible rows ahead of expanded rows like the dashboard', () => {
    const snapshot = codexState.snapshot!;
    const settings = settingsState.settings;
    const layout = settings.providers[0];
    const metric = (id: string) => layout.metrics.find((item) => item.id === id)!;
    const interleaved = {
      ...layout,
      expanded: true,
      metrics: [
        metric('codex.today'),
        metric('codex.session'),
        metric('codex.yesterday'),
        metric('codex.weekly'),
      ],
    };

    const rows = buildProviderShareRows('codex', snapshot, interleaved, settings, Date.now());
    expect(rows.map((row) => row.label)).toEqual(['Session', 'Weekly', 'Today', 'Yesterday']);
  });

  it('grows provider exports with content instead of enforcing a minimum canvas', () => {
    const settings = settingsState.settings;
    const layout = { ...settings.providers[0], expanded: true };
    const rows = buildProviderShareRows(
      'codex',
      codexState.snapshot!,
      layout,
      settings,
      Date.now(),
    );

    expect(providerShareCardHeight(rows)).toBeGreaterThan(
      providerShareCardHeight(rows.slice(0, 1)),
    );
    expect(providerShareCardHeight([])).toBeLessThan(providerShareCardHeight(rows));
  });

  it('keeps Total Spend to the period switcher and usage body', () => {
    expect(TOTAL_SPEND_PERIOD_LABELS).toEqual(['Today', 'Yesterday', '30 Days']);
    expect(TOTAL_SPEND_OUTER_PADDING).toBe(10);
    expect(TOTAL_SPEND_GEOMETRY).toMatchObject({
      width: 320,
      switcherHeight: 27,
      ringDiameter: 104,
      legendGap: 18,
    });
    expect(totalSpendShareCardHeight()).toBe(187);
  });

  it('shares the same geometry source with the live Total Spend card', () => {
    expect(totalSpendSource).toContain("import { TOTAL_SPEND_GEOMETRY } from './shareCard';");
    expect(totalSpendSource).toContain('--total-switcher-height:');
    expect(totalSpendSource).toContain('--total-ring-size:');
    expect(totalSpendSource).toContain('ringSectorPath(segment, TOTAL_SPEND_GEOMETRY)');
  });

  it('does not add a title, selected-period caption, or marketing footer to Total Spend', () => {
    const drawn: string[] = [];
    const context = {
      scale: vi.fn(),
      fillRect: vi.fn(),
      beginPath: vi.fn(),
      roundRect: vi.fn(),
      fill: vi.fn(),
      arc: vi.fn(),
      moveTo: vi.fn(),
      lineTo: vi.fn(),
      quadraticCurveTo: vi.fn(),
      closePath: vi.fn(),
      stroke: vi.fn(),
      measureText: (value: string) => ({ width: value.length * 6 }),
      fillText: (value: string) => drawn.push(value),
      textAlign: 'left',
      textBaseline: 'alphabetic',
      fillStyle: '',
      strokeStyle: '',
      font: '',
      lineWidth: 1,
      lineCap: 'butt',
    };
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(
      context as unknown as CanvasRenderingContext2D,
    );

    const canvas = renderTotalSpendShareCard({
      projection: {
        slices: [
          {
            id: 'codex',
            value: 12,
            period: {
              tokens: 1_000_000,
              estimatedCostUsd: 12,
              costEstimated: true,
              estimateComplete: true,
            },
          },
        ],
        centerValue: 12,
        costEstimated: true,
        estimateComplete: true,
      },
      metric: 'cost',
      period: 'last30Days',
    });

    expect(canvas.width).toBe(TOTAL_SPEND_GEOMETRY.width * SHARE_CARD_SCALE);
    expect(canvas.height).toBe(totalSpendShareCardHeight() * SHARE_CARD_SCALE);
    expect(drawn).toEqual(
      expect.arrayContaining(['Today', 'Yesterday', '30 Days', 'Codex', 'dollars']),
    );
    expect(drawn).not.toEqual(
      expect.arrayContaining([
        'Cost',
        'Last 30 Days',
        'Monitor Your AI Subscriptions with OpenQuota',
      ]),
    );
  });
});
