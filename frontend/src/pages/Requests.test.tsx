import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { fetchRequestDetail, fetchRequests } from '@/lib/api';
import { RequestsPage } from './Requests';

vi.mock('@/lib/api', () => ({ fetchRequests: vi.fn(), fetchRequestDetail: vi.fn() }));

const mockedFetchRequests = vi.mocked(fetchRequests);
const mockedFetchDetail = vi.mocked(fetchRequestDetail);

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}><LocaleProvider><RequestsPage /></LocaleProvider></QueryClientProvider>);
}

const sampleRow = {
  id: 'req-1',
  ts: Date.UTC(2026, 2, 18, 8, 30, 0),
  model: 'gpt-4o',
  pool_id: 'openai',
  key_hash: 'abc123def456',
  tenant_id: 'default',
  status_code: '200',
  prompt_tokens: '12',
  completion_tokens: '34',
  cached_tokens: '0',
  finish_reason: 'stop',
  error_code: null,
  latency_ms: 420,
  ttft_ms: '80ms',
  retry_count: 0,
  is_stream: false,
  cost_usd: 0.0123,
};

describe('request history page', () => {
  beforeEach(() => {
    localStorage.setItem('dashboard-locale', 'en');
    mockedFetchRequests.mockReset();
    mockedFetchDetail.mockReset();
  });

  it('shows a loading state while request history is pending', () => {
    mockedFetchRequests.mockReturnValue(new Promise(() => undefined));
    renderPage();
    expect(screen.getByLabelText('Loading')).toBeInTheDocument();
  });

  it('shows an error state when request history fails', async () => {
    mockedFetchRequests.mockRejectedValue(new Error('request failed'));
    renderPage();
    expect(await screen.findByText('Unable to load data')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
  });

  it('shows the empty state for a successful empty result', async () => {
    mockedFetchRequests.mockResolvedValue({ rows: [], filter: {}, tenants: [], total: 0, has_more: false });
    renderPage();
    expect(await screen.findByText('No requests in this window')).toBeInTheDocument();
  });

  it('renders a TTFT column ahead of latency', async () => {
    mockedFetchRequests.mockResolvedValue({ rows: [sampleRow], filter: { hours: 0 }, tenants: ['default'], total: 1, has_more: false });
    renderPage();
    expect(await screen.findByText('gpt-4o')).toBeInTheDocument();

    const headers = screen.getAllByRole('columnheader').map((h) => h.textContent);
    expect(headers).toEqual(['Time', 'Model', 'Pool', 'Status', 'Tokens (P/C/Cached)', 'TTFT', 'Latency', 'Finish', 'Error', 'Cost']);

    const cells = screen.getAllByRole('cell').map((c) => c.textContent);
    expect(cells).toContain('80ms');
    expect(cells).toContain('420ms');
  });

  it('renders a dash in the TTFT column for one-shot requests without TTFT', async () => {
    mockedFetchRequests.mockResolvedValue({
      rows: [{ ...sampleRow, ttft_ms: null }],
      filter: { hours: 0 },
      tenants: ['default'],
      total: 1,
      has_more: false,
    });
    renderPage();
    expect(await screen.findByText('gpt-4o')).toBeInTheDocument();

    const ttftIndex = screen.getAllByRole('columnheader').findIndex((h) => h.textContent === 'TTFT');
    const row = screen.getAllByRole('row')[1];
    expect(within(row).getAllByRole('cell')[ttftIndex].textContent).toBe('—');
  });

  it('lists each logged request and opens detail', async () => {
    mockedFetchRequests.mockResolvedValue({
      rows: [sampleRow],
      filter: { hours: 0 },
      tenants: ['default'],
      total: 1,
      has_more: false,
    });
    mockedFetchDetail.mockResolvedValue({
      ...sampleRow,
      client_ip: '127.0.0.1',
      upstream: 'https://api.openai.com',
      status_code: 200,
      latency_ms: 420,
      prompt_tokens: 12,
      completion_tokens: 34,
      total_tokens: 46,
      is_stream: false,
      error: null,
      cached_tokens: 0,
      cache_creation_tokens: null,
      cache_source: null,
      reasoning_tokens: null,
      audio_tokens: null,
      ttft_ms: 80,
      upstream_model: 'gpt-4o-2024-08-06',
      system_fingerprint: null,
      user_agent: 'curl/8.0',
    });
    renderPage();
    expect(await screen.findByText('gpt-4o')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Request History' })).toBeInTheDocument();
    expect(screen.getByText('openai')).toBeInTheDocument();
    fireEvent.click(screen.getByText('gpt-4o'));
    expect(await screen.findByText('Request detail')).toBeInTheDocument();
    expect(await screen.findByText('req-1')).toBeInTheDocument();
    expect(screen.getByText('abc123def456')).toBeInTheDocument();
  });
});
