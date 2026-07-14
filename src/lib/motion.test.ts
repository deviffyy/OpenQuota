import { describe, expect, it } from 'vitest';
import { reorderFlip, springEasing, springMotion } from './motion';

describe('shared motion profile', () => {
  it('matches the normalized endpoints of the shared spring', () => {
    expect(springEasing(0)).toBe(0);
    expect(springEasing(1)).toBe(1);
    expect(springEasing(0.55)).toBeGreaterThan(1);
  });

  it('uses the shared response and disables movement for reduced motion', () => {
    expect(reorderFlip(false).duration).toBe(630);
    expect(reorderFlip(true).duration).toBe(0);
    expect(reorderFlip(false).easing).toBe(springEasing);
    expect(reorderFlip).toBe(springMotion);
  });

  it('stays finite throughout the animation', () => {
    const samples: number[] = [];
    for (let step = 0; step <= 100; step += 1) {
      const value = springEasing(step / 100);
      samples.push(value);
      expect(Number.isFinite(value)).toBe(true);
    }
    expect(Math.min(...samples)).toBeGreaterThanOrEqual(0);
    expect(Math.max(...samples)).toBeLessThan(1.02);
  });
});
