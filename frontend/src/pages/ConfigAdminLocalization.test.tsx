import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { vi } from 'vitest';
import { beforeEach, describe, expect, it } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { SystemSettingsPage } from './settings/SystemSettingsPage';

const get = vi.fn();
vi.mock('@/lib/api', () => ({ api: { get: (...args: unknown[]) => get(...args) } }));

function renderLocale(page: React.ReactNode, locale: 'en' | 'zh-CN') {
  localStorage.setItem('dashboard-locale', locale);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}><LocaleProvider>{page}</LocaleProvider></QueryClientProvider>);
}

describe('configuration admin localization', () => {
  beforeEach(() => {
    localStorage.clear();
    get.mockReset();
    get.mockResolvedValue({
      data: {
        uptime_secs: 1,
        requests_total: 2,
        requests_failed: 0,
        cache_hits: 0,
        active_connections: 0,
        retries: 0,
        key_demotions: 0,
        upstream_5xx: 0,
        upstream_4xx: 0,
        alert_count: 0,
      },
    });
  });

  it('localizes system settings labels', async () => {
    renderLocale(<SystemSettingsPage />, 'en');
    expect(await screen.findByRole('heading', { name: 'System' })).toBeInTheDocument();
    expect(screen.getByText('Gateway status and configuration.')).toBeInTheDocument();
  });
});
