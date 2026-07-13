const SPRING_RESPONSE_SECONDS = 0.42;
const SPRING_DAMPING = 0.8;
const SPRING_SETTLE_RESPONSES = 1.5;
const TAU = Math.PI * 2;

function springStep(progress: number) {
  const clamped = Math.min(1, Math.max(0, progress));
  const responseTime = clamped * SPRING_SETTLE_RESPONSES;
  const dampedFrequency = Math.sqrt(1 - SPRING_DAMPING ** 2);
  const phase = TAU * dampedFrequency * responseTime;
  const envelope = Math.exp(-SPRING_DAMPING * TAU * responseTime);
  return 1 - envelope * (Math.cos(phase) + (SPRING_DAMPING / dampedFrequency) * Math.sin(phase));
}

const springEnd = springStep(1);

/** SwiftUI-style response 0.42 / damping 0.80 spring, normalized to finish exactly at 1. */
export function springEasing(progress: number) {
  if (progress <= 0) return 0;
  if (progress >= 1) return 1;
  return springStep(progress) / springEnd;
}

export function springMotion(reducedMotion: boolean) {
  return {
    duration: reducedMotion ? 0 : SPRING_RESPONSE_SECONDS * SPRING_SETTLE_RESPONSES * 1000,
    easing: springEasing,
  };
}

export const reorderFlip = springMotion;
