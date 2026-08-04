import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { fetchCost } from '@/lib/api';
import { CostPage } from './Cost';

vi.mock('@/lib/api', () => ({
  fetchCost: vi.fn(),
}));

const mockedFetchCost = vi.mocked(fetchCost);

function renderCost() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <LocaleProvider><CostPage /></LocaleProvider>
    </QueryClientProvider>,
  );
}

describe('monitoring data states', () => {
  beforeEach(() => {
    mockedFetchCost.mockReset();
  });

  it('shows a loading state while cost data is pending', () => {
    mockedFetchCost.mockReturnValue(new Promise(() => undefined));
    renderCost();
    expect(screen.getByLabelText('Loading')).toBeInTheDocument();
  });

  it('shows an error state when cost data fails', async () => {
    mockedFetchCost.mockRejectedValue(new Error('request failed'));
    renderCost();
    expect(await screen.findByText('Unable to load data')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
  });

  it('shows the empty state for a successful empty result', async () => {
    mockedFetchCost.mockResolvedValue({
      stats: {
        total_requests: '0',
        total_cost: '$0.00',
        avg_latency: '0ms',
        error_rate: '0%',
        total_errors: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: 0,
      },
      rows: [],
      tenants: [],
      selected_tenant: null,
    });
    renderCost();
    expect(await screen.findByText('No data — waiting for hourly aggregation')).toBeInTheDocument();
  });
});
