import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import StatusMetric from './StatusMetric.svelte';

afterEach(cleanup);

describe('StatusMetric', () => {
  it('renders semantic status text and its provider subtitle', () => {
    render(StatusMetric, {
      label: 'Extra Usage',
      metric: {
        id: 'payAsYouGo',
        label: 'Extra Usage',
        text: '2500 cap',
        tone: 'positive',
        subtitle: 'Pay as you go is enabled.',
      },
    });

    expect(screen.getByText('2500 cap')).toHaveClass('status-badge--positive');
    expect(screen.getByText('2500 cap')).toHaveAttribute(
      'data-tooltip',
      'Pay as you go is enabled.',
    );
  });

  it('shows an honest placeholder when the status is absent', () => {
    render(StatusMetric, { label: 'Extra Usage', metric: null });
    expect(screen.getByText('No data')).toBeInTheDocument();
  });
});
