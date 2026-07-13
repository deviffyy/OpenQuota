import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import SelectMenu from './SelectMenu.svelte';

const options = [
  { value: 'cost', label: 'Cost' },
  { value: 'tokens', label: 'Tokens' },
];

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('SelectMenu', () => {
  it('opens an application-styled listbox and selects an option', async () => {
    const onChange = vi.fn();
    render(SelectMenu, { label: 'Metric', value: 'cost', options, onChange });

    await fireEvent.click(screen.getByRole('combobox', { name: 'Metric' }));
    const listbox = screen.getByRole('listbox', { name: 'Metric' });
    expect(listbox).toBeInTheDocument();
    expect(listbox.parentElement).toBe(document.body);
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

  it('opens above the trigger when the visible area below is too small', async () => {
    vi.spyOn(window, 'innerHeight', 'get').mockReturnValue(200);
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: Element,
    ) {
      if (this.getAttribute('role') === 'combobox') {
        return {
          top: 160,
          right: 300,
          bottom: 188,
          left: 188,
          width: 112,
          height: 28,
          x: 188,
          y: 160,
          toJSON: () => ({}),
        };
      }
      return {
        top: 0,
        right: 0,
        bottom: 0,
        left: 0,
        width: 0,
        height: 0,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      };
    });

    render(SelectMenu, { label: 'Metric', value: 'cost', options, onChange: vi.fn() });
    await fireEvent.click(screen.getByRole('combobox', { name: 'Metric' }));

    expect(screen.getByRole('listbox', { name: 'Metric' })).toHaveClass('select-menu__list--above');
  });
});
