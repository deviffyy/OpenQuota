import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import RenameProviderSheet from './RenameProviderSheet.svelte';

afterEach(cleanup);

describe('rename provider sheet', () => {
  it('starts with the stored rename selected and submits the edited value', async () => {
    const onRename = vi.fn();
    render(RenameProviderSheet, {
      initialValue: 'Work',
      onRename,
      onCancel: vi.fn(),
    });

    const input = screen.getByRole('textbox', { name: 'Name' });
    await waitFor(() => expect(input).toHaveFocus());
    expect(input).toHaveValue('Work');
    await fireEvent.input(input, { target: { value: 'Personal' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    expect(onRename).toHaveBeenCalledWith('Personal');
  });

  it('returns an empty name for reset and cancels with Escape', async () => {
    const onRename = vi.fn();
    const onCancel = vi.fn();
    render(RenameProviderSheet, { initialValue: '', onRename, onCancel });

    await fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    expect(onRename).toHaveBeenCalledWith('');
    await fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(onCancel).toHaveBeenCalledOnce();
  });
});
