import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import UsageMetric from './UsageMetric.svelte';

describe('UsageMetric model detail', () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it('reveals the ranked real model names after the reference hover dwell', async () => {
    vi.useFakeTimers();
    render(UsageMetric, {
      label: 'Today',
      period: {
        tokens: 2_000,
        estimatedCostUsd: 0.04,
        costEstimated: true,
        estimateComplete: true,
        unknownModels: [],
        modelBreakdown: {
          sourceNote: 'From your Codex logs (estimated)',
          models: [
            { model: 'gpt-5.4', totalTokens: 1_100, costUsd: 0.03 },
            { model: 'gpt-5.3-codex', totalTokens: 900, costUsd: 0.01 },
          ],
        },
      },
    });

    const reading = screen.getByRole('button', { name: '$0.04 · 2K tokens' });
    await fireEvent.mouseEnter(reading);
    expect(screen.queryByRole('tooltip', { name: 'Today model usage' })).not.toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(400);

    const detail = screen.getByRole('tooltip', { name: 'Today model usage' });
    expect(detail).toHaveTextContent('gpt-5.4');
    expect(detail).toHaveTextContent('gpt-5.3-codex');
    expect(detail).toHaveTextContent('75%');
    expect(detail).toHaveTextContent('25%');
  });

  it('shows the unknown model warning without inventing a model breakdown', () => {
    render(UsageMetric, {
      label: 'Today',
      period: {
        tokens: 0,
        estimatedCostUsd: null,
        costEstimated: true,
        estimateComplete: false,
        unknownModels: ['future-unpriced-model'],
        modelBreakdown: null,
      },
    });

    expect(screen.getByLabelText('This period used a model with unknown pricing')).toHaveAttribute(
      'data-tooltip',
      'Unknown model found\n- future-unpriced-model',
    );
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('keeps incomplete cost text ordinary and reports local estimation separately', () => {
    render(UsageMetric, {
      label: 'Today',
      period: {
        tokens: 500,
        estimatedCostUsd: 0.03,
        costEstimated: true,
        estimateComplete: false,
        unknownModels: ['future-unpriced-model'],
        modelBreakdown: null,
      },
    });

    const reading = screen.getByRole('button', { name: '$0.03 · 500 tokens' });
    expect(reading).toHaveAttribute(
      'data-tooltip',
      '$0.03\n500 tokens\nEstimated locally, so it may be off',
    );
    expect(reading).not.toHaveTextContent('~');
    expect(screen.getByLabelText('This period used a model with unknown pricing')).toBeVisible();
  });

  it('compacts large row values while keeping exact tooltip figures', () => {
    render(UsageMetric, {
      label: 'Last 30 Days',
      period: {
        tokens: 1_506_025_363,
        estimatedCostUsd: 2_059.07,
        costEstimated: true,
        estimateComplete: true,
        unknownModels: [],
        modelBreakdown: null,
      },
    });

    expect(screen.getByRole('button', { name: '$2.1K · 1.5B tokens' })).toHaveAttribute(
      'data-tooltip',
      '$2,059.07\n1,506,025,363 tokens\nEstimated locally, so it may be off',
    );
  });

  it('lets the model detail replace the generic estimate tooltip', () => {
    render(UsageMetric, {
      label: 'Today',
      period: {
        tokens: 500,
        estimatedCostUsd: 0.03,
        costEstimated: true,
        estimateComplete: true,
        unknownModels: [],
        modelBreakdown: {
          sourceNote: 'From local logs (estimated)',
          models: [{ model: 'gpt-5.4', totalTokens: 500, costUsd: 0.03 }],
        },
      },
    });

    expect(screen.getByRole('button', { name: '$0.03 · 500 tokens' })).not.toHaveAttribute(
      'data-tooltip',
    );
  });
});
