export const PANEL_MIN_HEIGHT = 200;
export const PANEL_SCREEN_FRACTION = 0.85;

export function shouldDeferPanelFit(screen: string, refreshing: boolean) {
  return screen === 'dashboard' && refreshing;
}

export function panelMaximumHeight(workAreaHeight: number) {
  if (!Number.isFinite(workAreaHeight) || workAreaHeight <= 0) return 680;
  return Math.max(PANEL_MIN_HEIGHT, Math.floor(workAreaHeight * PANEL_SCREEN_FRACTION));
}

export function panelTargetHeight(idealHeight: number, workAreaHeight: number) {
  const maximum = panelMaximumHeight(workAreaHeight);
  return Math.min(maximum, Math.max(PANEL_MIN_HEIGHT, Math.ceil(idealHeight)));
}

export function screenPanelHeight(
  screen: 'dashboard' | 'customize' | 'settings' | `provider:${string}`,
  contentTarget: number,
  dashboardHeight: number,
) {
  return screen === 'settings' ? dashboardHeight : contentTarget;
}
