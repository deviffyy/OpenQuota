import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import SelectMenu from './SelectMenu.svelte';

const options = [
  { value: 'cost', label: 'Cost' },
  { value: 'tokens', label: 'Tokens' },
];

afterEach(cleanup);

describe('SelectMenu', () => {
  it('opens an application-styled listbox and selects an option', async () => {
    const onChange = vi.fn();
    render(SelectMenu, { label: 'Metric', value: 'cost', options, onChange });

    await fireEvent.click(screen.getByRole('combobox', { name: 'Metric' }));
    expect(screen.getByRole('listbox', { name: 'Metric' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Cost' })).toHaveAttribute('aria-selected', 'true');

    await fireEvent.click(screen.getByRole('option', { name: 'Tokens' }));
    expect(onChange).toHaveBeenCalledWith('tokens');
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('supports arrow navigation and Escape', async () => {
    render(SelectMenu, { label: 'Metric', value: 'cost', options, onChange: vi.fn() });
    const trigger = screen.getByRole('combobox', { name: 'Metric' });

    await fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    expect(screen.getByRole('option', { name: 'Cost' })).toHaveFocus();
    await fireEvent.keyDown(document.activeElement!, { key: 'ArrowDown' });
    expect(screen.getByRole('option', { name: 'Tokens' })).toHaveFocus();
    await fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
    expect(trigger).toHaveFocus();
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  });
});
