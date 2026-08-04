import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { vi } from 'vitest';
import { beforeEach, describe, expect, it } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { RoleListPage } from './admin/RoleListPage';
import { UserListPage } from './admin/UserListPage';
import { AuditLogPage } from './settings/AuditLogPage';

vi.mock('@/lib/api', () => ({ api: { get: vi.fn(() => new Promise(() => undefined)) } }));

function renderLocale(page: React.ReactNode, locale: 'en' | 'zh-CN') {
  localStorage.setItem('dashboard-locale', locale);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}><LocaleProvider>{page}</LocaleProvider></QueryClientProvider>);
}

describe('configuration admin localization', () => {
  beforeEach(() => localStorage.clear());

  it('localizes the built-in role page', () => {
    renderLocale(<RoleListPage />, 'en');
    expect(screen.getByRole('heading', { name: 'Roles' })).toBeInTheDocument();
    expect(screen.getByText('Built-in RBAC roles and their permissions.')).toBeInTheDocument();
  });

  it('localizes admin user and system labels', () => {
    renderLocale(<UserListPage />, 'en');
    expect(screen.getByRole('heading', { name: 'Users' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Add user/ })).toBeInTheDocument();

  });

  it('localizes audit availability guidance', () => {
    renderLocale(<AuditLogPage />, 'zh-CN');
    expect(screen.getByRole('heading', { name: '审计日志' })).toBeInTheDocument();
    expect(screen.getByText('审计事件仍可通过管理 API 使用。')).toBeInTheDocument();
  });
});
