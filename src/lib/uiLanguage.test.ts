import { describe, expect, it } from 'vitest';
import layoutCss from '../styles/layout.css?raw';
import sharedComponentCss from '../styles/components.css?raw';
import tokensCss from '../styles/tokens.css?raw';
import customizeDetail from './CustomizeProviderDetail.svelte?raw';
import customizeList from './CustomizeProviderList.svelte?raw';
import dashboard from './Dashboard.svelte?raw';
import settings from './SettingsScreen.svelte?raw';

const css = `${tokensCss}\n${layoutCss}\n${sharedComponentCss}`;

describe('native UI language contract', () => {
  it('uses the platform system font and reference type sizes', () => {
    expect(css).toMatch(/font-family:\s*system-ui,/);
    expect(css).not.toMatch(/font-family:\s*Inter/);
    expect(css).toMatch(/\.provider-header h1\s*{[^}]*font-size: 14px;[^}]*font-weight: 600;/s);
    expect(css).toMatch(/\.provider-list-main b\s*{[^}]*font-size: 14px;[^}]*font-weight: 600;/s);
    expect(css).toMatch(/\.setting-row\s*{[^}]*font-size: 13px;/s);
  });

  it('keeps the critical flame colored while its warning copy stays secondary', () => {
    expect(css).not.toMatch(/\.metric__heading span\s*{/);
    expect(css).toMatch(
      /\.metric__heading \.pace-warning__icon\s*{[^}]*color: var\(--meter-critical\);/s,
    );
    expect(css).toMatch(/\.metric__heading \.pace-warning\s*{[^}]*color: var\(--secondary\);/s);
  });

  it('keeps Customize concise and free of duplicate status and count copy', () => {
    expect(customizeList).toContain('Notifications, appearance and more');
    expect(customizeList).toContain('{provider.metrics.length} metrics');
    expect(customizeList).not.toContain('Detected locally');
    expect(customizeList).not.toContain('screen-intro');
    expect(customizeList).not.toContain('pinned\n');
    expect(customizeDetail).toContain('Drag metrics here');
    expect(customizeDetail).toContain('Starred for menu bar');
    expect(customizeDetail).toContain('Removed from menu bar');
    expect(customizeDetail).toContain('Up to 2 stars per provider');
    expect(customizeDetail).not.toContain('provider-toggle-row');
    expect(customizeDetail).not.toContain('section-divider');
    expect(customizeDetail).not.toContain('of 2 pinned');
  });

  it('uses the shared Settings labels and single-line control rows', () => {
    for (const label of [
      'General',
      'Show Total Spend',
      'Launch at Login',
      'Global Shortcut',
      'Icon Style',
      'Appearance',
      'Usage Display',
      'Notifications',
      'Advanced',
      'Updates',
      'Check for Updates Automatically',
      'Check for Updates…',
    ]) {
      expect(settings).toContain(label);
    }
    expect(settings).toContain("{ value: 'system', label: 'Auto' }");
    expect(settings).toContain("{ value: 'twelveHour', label: '12-hour' }");
    expect(settings).toContain("{ value: 'twentyFourHour', label: '24-hour' }");
    expect(settings).not.toContain('<h2>Startup</h2>');
    expect(settings).not.toContain('Automatic Checks');
    expect(settings).not.toContain('Combined cost and token summary.');
    expect(settings).not.toContain('Show projections even when usage is healthy.');
    expect(settings).not.toContain('>×</button');
  });

  it('keeps dashboard onboarding, empty state, and menus on the shared wording', () => {
    expect(dashboard).toContain('Welcome to OpenQuota');
    expect(dashboard).toContain('Open Customize');
    expect(dashboard).toContain('Turn on Customize to choose what to show.');
    expect(dashboard).toContain('Customize…');
    expect(dashboard).toContain('Refresh {providerDisplayName(menuProvider.id)}');
    expect(dashboard).not.toContain('Providers Detected');
    expect(dashboard).not.toContain('Starter Provider');
    expect(dashboard).not.toContain("Expand'} On Demand");
    expect(dashboard).not.toContain('>×</button');
  });
});
