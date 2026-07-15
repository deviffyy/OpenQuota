import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import ProviderNoticeRow from './ProviderNoticeRow.svelte';

describe('ProviderNoticeRow', () => {
  it('renders a compact warning status without hiding its retry context', () => {
    render(ProviderNoticeRow, {
      notice: {
        id: 'rateLimited',
        title: 'Live usage paused',
        message: 'Retrying in about 5 minutes',
        tone: 'warning',
      },
    });

    expect(screen.getByRole('status')).toHaveTextContent(
      'Live usage pausedRetrying in about 5 minutes',
    );
  });
});
