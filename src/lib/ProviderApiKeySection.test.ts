import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ProviderApiKeySection from './ProviderApiKeySection.svelte';

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));

describe('ProviderApiKeySection', () => {
  beforeEach(() => {
    mocks.invoke.mockReset().mockImplementation((command: string) => {
      if (command === 'get_provider_api_key_state') {
        return Promise.resolve({ providerId: 'openrouter', status: 'notSet' });
      }
      if (command === 'save_provider_api_key') {
        return Promise.resolve({ providerId: 'openrouter', status: 'saved' });
      }
      if (command === 'delete_provider_api_key') {
        return Promise.resolve({ providerId: 'openrouter', status: 'notSet' });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
  });

  afterEach(cleanup);

  it('saves a new key through the provider capability without rendering it afterward', async () => {
    render(ProviderApiKeySection, {
      providerId: 'openrouter',
      providerName: 'OpenRouter',
    });
    expect(await screen.findByRole('region', { name: 'OpenRouter API Key' })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    const input = screen.getByLabelText('OpenRouter API key');
    expect(input).toHaveAttribute('type', 'password');
    await fireEvent.input(input, { target: { value: 'sk-or-secret' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith('save_provider_api_key', {
        providerId: 'openrouter',
        apiKey: 'sk-or-secret',
      }),
    );
    expect(screen.queryByDisplayValue('sk-or-secret')).not.toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'OpenRouter API key source' })).toHaveValue(
      'Saved securely',
    );
  });

  it('offers an override for environment keys and clear for saved overrides', async () => {
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_provider_api_key_state') {
        return Promise.resolve({ providerId: 'openrouter', status: 'fromEnvironment' });
      }
      if (command === 'save_provider_api_key') {
        return Promise.resolve({ providerId: 'openrouter', status: 'overrideActive' });
      }
      if (command === 'delete_provider_api_key') {
        return Promise.resolve({ providerId: 'openrouter', status: 'fromEnvironment' });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    render(ProviderApiKeySection, {
      providerId: 'openrouter',
      providerName: 'OpenRouter',
    });
    await screen.findByRole('region', { name: 'OpenRouter API Key' });
    await fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
    expect(screen.getByRole('textbox', { name: 'OpenRouter API key source' })).toHaveValue(
      'From Your Environment',
    );
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Override With a Custom Key' }));
    await fireEvent.input(screen.getByLabelText('OpenRouter API key'), {
      target: { value: 'override' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByRole('textbox', { name: 'OpenRouter API key source' })).toHaveValue(
      'Custom Key',
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Remove saved API key' }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith('delete_provider_api_key', {
        providerId: 'openrouter',
      }),
    );
    expect(screen.getByRole('textbox', { name: 'OpenRouter API key source' })).toHaveValue(
      'From Your Environment',
    );
  });

  it('stays absent for providers without the API-key capability', async () => {
    mocks.invoke.mockResolvedValue(null);
    render(ProviderApiKeySection, {
      providerId: 'codex',
      providerName: 'Codex',
    });
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith('get_provider_api_key_state', {
        providerId: 'codex',
      }),
    );
    expect(screen.queryByRole('region', { name: 'Codex API Key' })).not.toBeInTheDocument();
  });

  it('shows an actionable credential-store error instead of hiding the API-key controls', async () => {
    mocks.invoke.mockRejectedValue(
      'Linux Secret Service is unavailable. Start or unlock your keyring and try again.',
    );
    render(ProviderApiKeySection, {
      providerId: 'openrouter',
      providerName: 'OpenRouter',
    });

    expect(await screen.findByRole('region', { name: 'OpenRouter API Key' })).toBeInTheDocument();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Linux Secret Service is unavailable. Start or unlock your keyring and try again.',
    );
    expect(screen.getByRole('button', { name: 'Add' })).toBeInTheDocument();
  });
});
