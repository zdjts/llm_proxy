import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import { LocaleProvider, useLocale } from '@/i18n/context';
import { PublicShell } from '@/components/site/PublicShell';
import { AccountShell } from '@/components/site/AccountShell';
import { ManifestPage } from '@/pages/site/ManifestPage';
import { accountRoutes, publicRoutes, routeManifest } from './manifest';

function LocaleControls() { const { setLocale } = useLocale(); return <button type="button" onClick={() => setLocale('zh-CN')}>中文</button>; }
function local(page: React.ReactNode) { localStorage.setItem('dashboard-locale', 'en'); return render(<LocaleProvider>{page}<LocaleControls /></LocaleProvider>); }

beforeEach(() => { localStorage.clear(); });

describe('site route architecture', () => {
  it('covers each declared route with a unique locale-neutral key and mode', () => {
    expect(new Set(routeManifest.map((route) => route.path)).size).toBe(routeManifest.length);
    expect(publicRoutes).toHaveLength(15);
    expect(accountRoutes.length).toBeGreaterThan(50);
    expect(routeManifest.every((route) => ['backed', 'unavailable', 'protected'].includes(route.mode))).toBe(true);
    expect(routeManifest.every((route) => !('title' in route) && !('description' in route))).toBe(true);
  });

  it('updates public navigation and menu accessible names after locale switching', () => {
    local(<MemoryRouter><Routes><Route element={<PublicShell />}><Route path="/" element={<h1>Product page</h1>} /></Route></Routes></MemoryRouter>);
    expect(screen.getByRole('navigation', { name: 'Public navigation' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '中文' }));
    expect(screen.getByRole('navigation', { name: '公开导航' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '打开导航' }));
    expect(screen.getByRole('navigation', { name: '公开移动导航' })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.getByRole('button', { name: '打开导航' })).toHaveFocus();
  });

  it('keeps login form visible after entering the console from the public shell', () => {
    local(
      <MemoryRouter initialEntries={['/']}>
        <Routes>
          <Route element={<PublicShell />}>
            <Route path="/" element={<h1>Product page</h1>} />
            <Route path="/login" element={<h1>Welcome Back</h1>} />
          </Route>
        </Routes>
      </MemoryRouter>,
    );

    const shell = document.querySelector('.public-shell');
    expect(shell).not.toBeNull();
    expect(shell?.classList.contains('home-page')).toBe(false);

    fireEvent.click(screen.getByRole('link', { name: 'Open console' }));
    expect(screen.getByRole('heading', { name: 'Welcome Back' })).toBeVisible();
  });

  it('localizes the network status route title and description', () => {
    const route = routeManifest.find((entry) => entry.key === 'networkStatus');
    local(<ManifestPage route={route!} />);
    expect(screen.getByRole('heading', { name: 'Network status' })).toBeInTheDocument();
    expect(screen.getByText('Review service network availability.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '中文' }));
    expect(screen.getByRole('heading', { name: '网络状态' })).toBeInTheDocument();
    expect(screen.getByText('查看服务网络可用性。')).toBeInTheDocument();
  });

  it('renders authored public page compositions without fake data', () => {
    const home = routeManifest.find((entry) => entry.key === 'home');
    const models = routeManifest.find((entry) => entry.key === 'models');
    local(<MemoryRouter><ManifestPage route={home!} /><ManifestPage route={models!} /></MemoryRouter>);
    expect(screen.getByRole('heading', { name: 'LLM Proxy' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Model directory' })).toBeInTheDocument();
    expect(screen.queryByText(/\$|balance|orders/i)).not.toBeInTheDocument();
  });

  it('keeps unavailable actions disabled in both locales and localizes route copy', () => {
    const route = routeManifest.find((entry) => entry.key === 'subscriptions');
    expect(route).toBeDefined();
    local(<ManifestPage route={route!} />);
    expect(screen.getByRole('button', { name: 'Action unavailable' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: '中文' }));
    expect(screen.getByRole('heading', { name: '订阅' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '操作暂不可用' })).toBeDisabled();
  });

  it('renders every grouped account route and closes the mobile menu safely', () => {
    local(<MemoryRouter initialEntries={['/account']}><Routes><Route element={<AccountShell />}><Route path="/account" element={<h1>Account page</h1>} /></Route></Routes></MemoryRouter>);
    for (const group of ['Workspace', 'Developer', 'Commercial', 'Community', 'Activities', 'Operator']) expect(screen.getByText(group)).toBeInTheDocument();
    for (const route of accountRoutes) expect(screen.getAllByRole('link').some((link) => link.getAttribute('href') === route.path)).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Open navigation' }));
    expect(screen.getByRole('navigation', { name: 'Account mobile navigation' })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.getByRole('button', { name: 'Open navigation' })).toHaveFocus();
  });

  it('renders commercial, community, and activity families as inert localized pages', () => {
    const commercial = routeManifest.find((entry) => entry.key === 'subscriptions');
    const community = routeManifest.find((entry) => entry.key === 'affiliate');
    const activities = routeManifest.find((entry) => entry.key === 'arena');
    local(<MemoryRouter><ManifestPage route={commercial!} /><ManifestPage route={community!} /><ManifestPage route={activities!} /></MemoryRouter>);
    expect(screen.getByRole('heading', { name: 'Subscriptions' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Affiliate' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Arena' })).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: 'Action unavailable' })).toHaveLength(3);
    expect(screen.getAllByRole('button', { name: 'Unavailable' })).toHaveLength(3);
    expect(screen.queryByText(/\$|balance|order|reward|coupon/i)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '中文' }));
    expect(screen.getByRole('heading', { name: '订阅' })).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: '操作暂不可用' })).toHaveLength(3);
  });

  it('resolves every operator route to a guarded inert template', () => {
    const operatorRoutes = routeManifest.filter((entry) => entry.group === 'operator');
    expect(operatorRoutes).toHaveLength(19);
    for (const route of operatorRoutes) {
      const view = render(<LocaleProvider><MemoryRouter><ManifestPage route={route} /></MemoryRouter></LocaleProvider>);
      expect(screen.getAllByRole('heading').length).toBeGreaterThan(0);
      expect(screen.getByRole('textbox')).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Filters unavailable' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Create unavailable' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Export unavailable' })).toBeDisabled();
      expect(screen.queryByText(/[$€¥]|\b(?:USD|CNY|balance:|payment success|reward:|rebate:)\b/i)).not.toBeInTheDocument();
      view.unmount();
    }
  });

  it('localizes operator unavailable controls', () => {
    const route = routeManifest.find((entry) => entry.key === 'operatorUsers');
    local(<ManifestPage route={route!} />);
    fireEvent.click(screen.getByRole('button', { name: '中文' }));
    expect(screen.getByRole('heading', { name: '运维用户' })).toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: '搜索暂不可用' })).toBeDisabled();
    expect(screen.getByRole('button', { name: '筛选暂不可用' })).toBeDisabled();
    expect(screen.getByRole('button', { name: '创建暂不可用' })).toBeDisabled();
    expect(screen.getByRole('button', { name: '导出暂不可用' })).toBeDisabled();
  });

  it('renders workspace and developer presentation pages with inert controls', () => {
    const profile = routeManifest.find((entry) => entry.key === 'profile');
    const channels = routeManifest.find((entry) => entry.key === 'availableChannels');
    local(<MemoryRouter><ManifestPage route={profile!} /><ManifestPage route={channels!} /></MemoryRouter>);
    expect(screen.getAllByText('No account data is available.')).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: 'Edit unavailable' })).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: 'Unavailable' })).toHaveLength(2);
    expect(screen.queryByText(/\$|balance|orders/i)).not.toBeInTheDocument();
  });
});
