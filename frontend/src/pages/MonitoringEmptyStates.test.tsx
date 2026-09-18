import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { fetchTraffic } from '@/lib/api';
import { TrafficPage } from './Traffic';

vi.mock('@/lib/api', () => ({ fetchTraffic: vi.fn() }));

function renderPage(page: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}><LocaleProvider>{page}</LocaleProvider></QueryClientProvider>);
}

describe('monitoring successful empty states', () => {
  beforeEach(() => {
    vi.mocked(fetchTraffic).mockReset();
  });

  it('shows an empty state without traffic shells for empty series', async () => {
    vi.mocked(fetchTraffic).mockResolvedValue({ chart: { labels: [], lines: [] }, days: 7, w: 0, tenants: [], selected_tenant: null });
    renderPage(<TrafficPage />);
    expect(await screen.findByText('No data')).toBeInTheDocument();
    expect(screen.queryByText('Request Volume')).not.toBeInTheDocument();
    expect(screen.queryByText('Total Requests')).not.toBeInTheDocument();
  });

  it('keeps valid zero-valued traffic in the populated layout', async () => {
    vi.mocked(fetchTraffic).mockResolvedValue({
      chart: { labels: [{ x: 0, text: '00:00' }], lines: [{ points: '0,200 10,200', color: '#000' }, { points: '0,200 10,200', color: '#000' }] },
      days: 7, w: 10, tenants: [], selected_tenant: null,
    });
    renderPage(<TrafficPage />);
    expect(await screen.findByText('Request Volume')).toBeInTheDocument();
    expect(screen.getByText('Total Requests')).toBeInTheDocument();
    expect(screen.queryByText('No data')).not.toBeInTheDocument();
  });
});
