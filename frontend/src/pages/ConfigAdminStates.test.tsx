import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { addClientKey, createPool, deleteProvider, fetchClientKeys, fetchPools, fetchProviders } from '@/lib/api';
import { ClientKeysPage } from './ClientKeys';
import { ProviderListPage } from './config/ProviderListPage';
import { PoolListPage } from './config/PoolListPage';

vi.mock('@/lib/api', () => ({
  addClientKey: vi.fn(), createPool: vi.fn(), deletePool: vi.fn(), deleteProvider: vi.fn(), fetchClientKeys: vi.fn(), fetchPools: vi.fn(), fetchProviders: vi.fn(),
  updateClientKey: vi.fn(), deleteClientKey: vi.fn(), rotateClientKey: vi.fn(),
  apiError: () => 'Request failed.',
}));

function renderPage(page: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}><LocaleProvider>{page}</LocaleProvider></QueryClientProvider>);
}

describe('configuration and admin states', () => {
  beforeEach(() => {
    vi.mocked(fetchProviders).mockReset();
    vi.mocked(deleteProvider).mockReset();
    vi.mocked(fetchClientKeys).mockReset();
    vi.mocked(addClientKey).mockReset();
    vi.mocked(fetchPools).mockReset();
    vi.mocked(createPool).mockReset();
    vi.stubGlobal('confirm', vi.fn(() => true));
  });

  it('keeps provider loading, error, empty and populated states exclusive', async () => {
    vi.mocked(fetchProviders).mockReturnValueOnce(new Promise(() => undefined));
    const { unmount } = renderPage(<ProviderListPage />);
    expect(screen.getByLabelText('Loading')).toBeInTheDocument();
    expect(screen.queryByText('No providers configured')).not.toBeInTheDocument();
    unmount();

    vi.mocked(fetchProviders).mockRejectedValueOnce(new Error('failed'));
    renderPage(<ProviderListPage />);
    expect(await screen.findByText('Unable to load data')).toBeInTheDocument();
    expect(screen.queryByText('No providers configured')).not.toBeInTheDocument();
  });

  it('shows empty and populated provider states, and guards deletion', async () => {
    vi.mocked(fetchProviders).mockResolvedValueOnce({ providers: [] });
    const { unmount } = renderPage(<ProviderListPage />);
    expect(await screen.findByText('No providers configured')).toBeInTheDocument();
    unmount();

    vi.mocked(fetchProviders).mockResolvedValueOnce({ providers: [{ id: 'p1', kind: 'openai', base_url: 'https://example.test', pool_id: 'pool' }] });
    vi.mocked(deleteProvider).mockResolvedValue({});
    renderPage(<ProviderListPage />);
    const button = await screen.findByRole('button', { name: 'Delete p1' });
    fireEvent.click(button);
    expect(window.confirm).toHaveBeenCalled();
    await waitFor(() => expect(deleteProvider).toHaveBeenCalled());
    expect(vi.mocked(deleteProvider).mock.calls[0][0]).toBe('p1');
  });

  it('masks and clears pool secrets across the create lifecycle', async () => {
    vi.mocked(fetchPools).mockResolvedValue({ pools: [] });
    vi.mocked(createPool).mockRejectedValueOnce(new Error('failed')).mockResolvedValueOnce({});
    renderPage(<PoolListPage />);

    fireEvent.click(await screen.findByRole('button', { name: /Add pool/i }));
    const secret = screen.getByPlaceholderText('API key');
    expect(secret).toHaveAttribute('type', 'password');
    fireEvent.change(secret, { target: { value: 'first-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    await waitFor(() => expect(createPool).toHaveBeenCalled());
    expect(vi.mocked(createPool).mock.calls[0][0]).toMatchObject({ keys: [{ key: 'first-secret', weight: 1 }] });
    await waitFor(() => expect(secret).toHaveValue(''));

    fireEvent.change(screen.getByPlaceholderText('API key'), { target: { value: 'close-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: /Add pool/i }));
    expect(screen.getByPlaceholderText('API key')).toHaveValue('');

    fireEvent.change(screen.getByPlaceholderText('API key'), { target: { value: 'success-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    await waitFor(() => expect(createPool).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByPlaceholderText('API key')).not.toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: /Add pool/i }));
    expect(screen.getByPlaceholderText('API key')).toHaveValue('');
  });

  it('masks and clears client secrets across cancel, failure, success, and reopen', async () => {
    vi.mocked(fetchClientKeys).mockResolvedValue({ keys: [], total: 0 });
    vi.mocked(addClientKey).mockRejectedValueOnce(new Error('failed')).mockResolvedValueOnce({ key_hash: 'hash', tenant_id: 'default', label: '', created_at: 0, enabled: true });
    renderPage(<ClientKeysPage />);

    fireEvent.click(await screen.findByRole('button', { name: 'Add Key' }));
    const secret = screen.getByPlaceholderText('API Key');
    expect(secret).toHaveAttribute('type', 'password');
    expect(secret).toHaveAttribute('autocomplete', 'new-password');
    fireEvent.change(secret, { target: { value: 'cancel-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add Key' }));
    expect(screen.getByPlaceholderText('API Key')).toHaveValue('');

    fireEvent.change(screen.getByPlaceholderText('API Key'), { target: { value: 'failed-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    await waitFor(() => expect(addClientKey).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByPlaceholderText('API Key')).toHaveValue(''));

    fireEvent.change(screen.getByPlaceholderText('API Key'), { target: { value: 'success-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    await waitFor(() => expect(addClientKey).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByPlaceholderText('API Key')).not.toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: 'Add Key' }));
    expect(screen.getByPlaceholderText('API Key')).toHaveValue('');
  });
});
