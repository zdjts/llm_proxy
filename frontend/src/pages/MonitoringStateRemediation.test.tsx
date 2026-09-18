import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { fetchCost, fetchKeys, fetchStatus, fetchTraffic } from '@/lib/api';
import { Overview } from './Overview';

vi.mock('@/lib/api', () => ({ fetchCost: vi.fn(), fetchKeys: vi.fn(), fetchStatus: vi.fn(), fetchTraffic: vi.fn(),
}));

function renderPage(page: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}><LocaleProvider>{page}</LocaleProvider></QueryClientProvider>);
}

describe('monitoring state remediation', () => {
  beforeEach(() => {
    vi.mocked(fetchStatus).mockReset();
    vi.mocked(fetchTraffic).mockReset();
    vi.mocked(fetchKeys).mockReset();
    vi.mocked(fetchCost).mockReset();
  });

  it('retries every failed overview query', async () => {
    const status = vi.mocked(fetchStatus).mockRejectedValueOnce(new Error('failed')).mockResolvedValueOnce({ uptime_secs: 0, requests_total: 0, requests_failed: 0, stream_requests: 0, cache_hits: 0, active_connections: 0, prompt_tokens: 0, completion_tokens: 0, retries: 0, key_demotions: 0, upstream_5xx: 0, upstream_4xx: 0, pools: [], alert_count: 0 });
    vi.mocked(fetchTraffic).mockResolvedValue({ chart: { labels: [], lines: [] }, days: 7, w: 0, tenants: [], selected_tenant: null });
    vi.mocked(fetchKeys).mockResolvedValue({ pools: [] });
    vi.mocked(fetchCost).mockResolvedValue({ stats: { total_requests: '0', total_cost: '$0', avg_latency: '0ms', error_rate: '0%', total_errors: 0, prompt_tokens: 0, completion_tokens: 0, cached_tokens: 0 }, rows: [], tenants: [], selected_tenant: null });
    renderPage(<Overview />);
    expect(await screen.findByText('Unable to load data')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(status).toHaveBeenCalledTimes(2));
  });
});
