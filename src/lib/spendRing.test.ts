import { describe, expect, it } from 'vitest';
import { ringSectorPath, spendRingArcs, type SpendRingGeometry } from './spendRing';

const geometry: SpendRingGeometry = {
  ringDiameter: 104,
  innerRadiusRatio: 0.618,
  gapWidth: 1.6,
  cornerRadius: 3,
};

describe('Total Spend ring sectors', () => {
  it('closes the normalized ring and preserves a visible minimum slice', () => {
    const arcs = spendRingArcs([
      { id: 'cursor', value: 7_121.12 },
      { id: 'claude', value: 6_042.64 },
      { id: 'codex', value: 134.46 },
      { id: 'grok', value: 71.02 },
    ]);

    const grok = arcs.find((arc) => arc.id === 'grok')!;
    expect(arcs[0].start).toBe(0);
    expect(arcs.at(-1)?.end).toBeCloseTo(1, 10);
    expect(grok.end - grok.start).toBeGreaterThanOrEqual(0.02);
  });

  it('creates filled annular sectors with rounded inner and outer corners', () => {
    const arcs = spendRingArcs([
      { id: 'large', value: 999 },
      { id: 'tiny', value: 1 },
    ]);

    for (const arc of arcs) {
      const path = ringSectorPath(arc, geometry);
      expect(path).toMatch(/^M .* A .* Q .* L .* Q .* A .* Q .* L .* Q .* Z$/);
      expect(path).not.toContain('NaN');
    }
  });
});
