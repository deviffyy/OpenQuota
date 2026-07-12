export const UPDATE_STARTUP_DELAY_MS = 10_000;
export const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

export function automaticUpdateDelay(lastSuccessfulCheck: string | null, now = Date.now()) {
  if (!lastSuccessfulCheck) return UPDATE_STARTUP_DELAY_MS;
  const checkedAt = new Date(lastSuccessfulCheck).getTime();
  if (!Number.isFinite(checkedAt)) return UPDATE_STARTUP_DELAY_MS;
  const remaining = UPDATE_CHECK_INTERVAL_MS - Math.max(0, now - checkedAt);
  return remaining <= 0 ? UPDATE_STARTUP_DELAY_MS : Math.max(1_000, remaining);
}
