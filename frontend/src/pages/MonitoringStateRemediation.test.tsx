import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { PropsWithChildren } from 'react';
import { LocaleProvider } from '@/i18n/context';
import { fetchAlerts, fetchCost, fetchKeys, fetchQuotas, fetchStatus, fetchTraffic } from '@/lib/api';
import { Overview } from './Overview';
import { QuotasPage } from './Quotas';

vi.mock('@/lib/api', () => ({
  fetchAlerts: vi.fn(), fetchCost: vi.fn(), fetchKeys: vi.fn(), fetchQuotas: vi.fn(), fetchStatus: vi.fn(), fetchTraffic: vi.fn(),
}));
vi.mock('recharts', () => ({
  Area: () => null, AreaChart: ({ children }: PropsWithChildren) => <div>{children}</div>, CartesianGrid: () => null,
  ResponsiveContainer: ({ children }: PropsWithChildren) => <div>{children}</div>, Tooltip: () => null, XAxis: () => null, YAxis: () => null,
}));

function renderPage(page: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}><LocaleProvider>{page}</LocaleProvider></QueryClientProvider>);
}

describe('monitoring state remediation', () => {
  beforeEach(() => {
    vi.mocked(fetchQuotas).mockReset();
    vi.mocked(fetchStatus).mockReset();
    vi.mocked(fetchTraffic).mockReset();
    vi.mocked(fetchAlerts).mockReset();
    vi.mocked(fetchKeys).mockReset();
    vi.mocked(fetchCost).mockReset();
  });

  it('renders only loading while quotas are pending', () => {
    vi.mocked(fetchQuotas).mockReturnValue(new Promise(() => undefined));
    renderPage(<QuotasPage />);
    expect(screen.getByLabelText('Loading')).toBeInTheDocument();
    expect(screen.queryByText('No quota data')).not.toBeInTheDocument();
  });

  it('renders only error and retries quotas', async () => {
    const failed = vi.mocked(fetchQuotas).mockRejectedValueOnce(new Error('failed')).mockResolvedValueOnce([]);
    renderPage(<QuotasPage />);
    expect(await screen.findByText('Unable to load data')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(failed.mock.calls.length).toBeGreaterThan(1));
    expect(screen.queryByText('Unable to load data')).not.toBeInTheDocument();
  });

  it('renders the empty state for successful empty quotas', async () => {
    vi.mocked(fetchQuotas).mockResolvedValue([]);
    renderPage(<QuotasPage />);
    expect(await screen.findByText('No quota data available')).toBeInTheDocument();
  });

  it('renders quota data instead of the empty state', async () => {
    vi.mocked(fetchQuotas).mockResolvedValue([{ tenant_id: 'acme', daily_tokens_used: 1, daily_tokens_limit: 10, monthly_requests_used: 1, monthly_requests_limit: 10 }]);
    renderPage(<QuotasPage />);
    expect(await screen.findByText('acme')).toBeInTheDocument();
    expect(screen.queryByText('No quota data')).not.toBeInTheDocument();
  });

  it('retries every failed overview query', async () => {
    const status = vi.mocked(fetchStatus).mockRejectedValueOnce(new Error('failed')).mockResolvedValueOnce({ uptime_secs: 0, requests_total: 0, requests_failed: 0, stream_requests: 0, cache_hits: 0, active_connections: 0, prompt_tokens: 0, completion_tokens: 0, retries: 0, key_demotions: 0, upstream_5xx: 0, upstream_4xx: 0, pools: [], alert_count: 0 });
    vi.mocked(fetchTraffic).mockResolvedValue({ chart: { labels: [], lines: [] }, days: 7, w: 0, tenants: [], selected_tenant: null });
    vi.mocked(fetchAlerts).mockResolvedValue({ event_count: 0, events: [], selected_type: null, tenant_filter: '', ts_from: '', ts_to: '' });
    vi.mocked(fetchKeys).mockResolvedValue({ pools: [] });
    vi.mocked(fetchCost).mockResolvedValue({ stats: { total_requests: '0', total_cost: '$0', avg_latency: '0ms', error_rate: '0%', total_errors: 0, prompt_tokens: 0, completion_tokens: 0, cached_tokens: 0 }, rows: [], tenants: [], selected_tenant: null });
    renderPage(<Overview />);
    expect(await screen.findByText('Unable to load data')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(status).toHaveBeenCalledTimes(2));
  });
});
