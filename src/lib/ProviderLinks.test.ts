import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ProviderLinks from './ProviderLinks.svelte';

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
});
