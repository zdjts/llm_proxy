import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { fetchAlerts, fetchCost, fetchKeys, fetchStatus, fetchTraffic } from '@/lib/api';
import type { AlertsResponse, CostResponse, KeysResponse, StatusResponse, TrafficResponse } from '@/types';
import { Overview } from './Overview';

vi.mock('@/lib/api', () => ({ fetchAlerts: vi.fn(), fetchCost: vi.fn(), fetchKeys: vi.fn(), fetchStatus: vi.fn(), fetchTraffic: vi.fn() }));
const mocks = { alerts: vi.mocked(fetchAlerts), cost: vi.mocked(fetchCost), keys: vi.mocked(fetchKeys), status: vi.mocked(fetchStatus), traffic: vi.mocked(fetchTraffic) };
function renderOverview() { const client = new QueryClient({ defaultOptions: { queries: { retry: false } } }); return render(<QueryClientProvider client={client}><LocaleProvider><Overview /></LocaleProvider></QueryClientProvider>); }
const empty = { days: 7, w: 0, chart: { labels: [], lines: [] }, tenants: [], selected_tenant: null } as TrafficResponse;
const statusEmpty = { uptime_secs: 0, requests_total: 0, stream_requests: 0, requests_failed: 0, prompt_tokens: 0, completion_tokens: 0, cache_hits: 0, active_connections: 0, retries: 0, key_demotions: 0, upstream_5xx: 0, upstream_4xx: 0, pools: [], alert_count: 0 } as StatusResponse;
const alertsEmpty = { event_count: 0, events: [], selected_type: null, tenant_filter: '', ts_from: '', ts_to: '' } as AlertsResponse;
const costEmpty = { stats: { total_requests: '0', total_cost: '$0.00', avg_latency: '0ms', error_rate: '0%', total_errors: 0, prompt_tokens: 0, completion_tokens: 0, cached_tokens: 0 }, rows: [], tenants: [], selected_tenant: null } as CostResponse;
const keysEmpty = { pools: [] } as KeysResponse;

beforeEach(() => { Object.values(mocks).forEach((mock) => mock.mockReset()); Object.defineProperty(globalThis, 'ResizeObserver', { configurable: true, value: class { observe() {} unobserve() {} disconnect() {} } }); });

describe('console overview composition', () => {
  it('keeps the data-backed metric strip and honest empty state', async () => {
    mocks.status.mockResolvedValue(statusEmpty);
    mocks.traffic.mockResolvedValue(empty);
    mocks.alerts.mockResolvedValue(alertsEmpty);
    mocks.keys.mockResolvedValue(keysEmpty);
    mocks.cost.mockResolvedValue(costEmpty);
    renderOverview();
    expect(await screen.findByRole('heading', { name: 'Dashboard Overview' })).toBeInTheDocument();
    expect(await screen.findByText('No pools configured')).toBeInTheDocument();
    expect(screen.getByText('Total Requests')).toBeInTheDocument();
    expect(screen.queryByText(/fake|sample|demo/i)).not.toBeInTheDocument();
  });

  it('renders one mutually exclusive error state when an overview query fails', async () => {
    mocks.status.mockRejectedValue(new Error('status unavailable'));
    mocks.traffic.mockResolvedValue(empty);
    mocks.alerts.mockResolvedValue(alertsEmpty);
    mocks.keys.mockResolvedValue(keysEmpty);
    mocks.cost.mockResolvedValue(costEmpty);
    renderOverview();
    expect(await screen.findByText('Unable to load data')).toBeInTheDocument();
    expect(screen.queryByText('Total Requests')).not.toBeInTheDocument();
  });
});
