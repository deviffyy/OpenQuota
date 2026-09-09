import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ProviderLinks from './ProviderLinks.svelte';
import { setUiLanguage } from './i18n';

describe('ProviderLinks', () => {
  it('opens the selected catalog link and caps the grid at three columns', async () => {
    const onOpen = vi.fn();
    const { container } = render(ProviderLinks, {
      links: [
        { label: 'Status', url: 'https://status.example.com/' },
        { label: 'Dashboard', url: 'https://example.com/dashboard' },
        { label: 'Docs', url: 'https://example.com/docs' },
        { label: 'Support', url: 'https://example.com/support' },
      ],
      onOpen,
    });

    expect(container.querySelector('.provider-links')).toHaveStyle('--provider-link-columns: 3');
    await fireEvent.click(screen.getByRole('button', { name: 'Docs, opens in browser' }));
    expect(onOpen).toHaveBeenCalledWith(2);
  });

  it('localizes app-owned link kinds and preserves custom labels', () => {
    setUiLanguage('zh-CN');
    const { container } = render(ProviderLinks, {
      links: [
        { label: 'Dashboard', url: 'https://example.com/', kind: 'dashboard' },
        { label: 'Docs', url: 'https://example.com/docs' },
      ],
      onOpen: vi.fn(),
    });

    const buttons = container.querySelectorAll('button');
    expect(buttons[0]).toHaveAccessibleName('控制台，将在浏览器中打开');
    expect(buttons[1]).toHaveAccessibleName('Docs，将在浏览器中打开');
    setUiLanguage('en');
  });
});
