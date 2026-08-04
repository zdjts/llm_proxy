import { HashRouter, Navigate, Route, Routes } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LocaleProvider, useLocale } from '@/i18n/context';
import { Layout } from '@/components/Layout';
import { RouteGuard } from '@/components/RouteGuard';
import { PublicShell } from '@/components/site/PublicShell';
import { AccountShell } from '@/components/site/AccountShell';
import { accountRoutes, publicRoutes } from '@/routes/manifest';
import { ManifestPage } from '@/pages/site/ManifestPage';
import { UnavailableState } from '@/components/site/UnavailableState';
import { LoginPage } from '@/pages/auth/LoginPage';
import { Overview } from '@/pages/Overview';
import { CostPage } from '@/pages/Cost';
import { RequestsPage } from '@/pages/Requests';
import { KeysPage } from '@/pages/Keys';
import { TrafficPage } from '@/pages/Traffic';
import { AlertsPage } from '@/pages/Alerts';
import { DrilldownPage } from '@/pages/Drilldown';
import { HelpPage } from '@/pages/Help';
import { LivePage } from '@/pages/Live';
import { ClientKeysPage } from '@/pages/ClientKeys';
import { QuotasPage } from '@/pages/Quotas';
import { ConfigConsolePage } from '@/pages/ConfigConsole';
import { ProviderListPage } from '@/pages/config/ProviderListPage';
import { PoolListPage } from '@/pages/config/PoolListPage';
import { RoutingConfigPage } from '@/pages/config/RoutingConfigPage';
import { ModelCatalogPage } from '@/pages/config/ModelCatalogPage';
import { BudgetPage } from '@/pages/budget/BudgetPage';
import { UserListPage } from '@/pages/admin/UserListPage';
import { RoleListPage } from '@/pages/admin/RoleListPage';
import { AuditLogPage } from '@/pages/settings/AuditLogPage';
import { SystemSettingsPage } from '@/pages/settings/SystemSettingsPage';

const queryClient = new QueryClient({ defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } } });

function FallbackPage() {
  const { t } = useLocale();
  return <UnavailableState title={t.site.fallback.title} description={t.site.fallback.description} />;
}

export default function App() {
  return <QueryClientProvider client={queryClient}><LocaleProvider><HashRouter><Routes>
    <Route element={<PublicShell />}>{publicRoutes.filter((route) => route.path !== '/login').map((route) => <Route key={route.path} path={route.path} element={<ManifestPage route={route} />} />)}<Route path="/login" element={<LoginPage />} /><Route path="*" element={<FallbackPage />} /></Route>
    <Route element={<RouteGuard />}><Route element={<AccountShell />}>{accountRoutes.map((route) => <Route key={route.path} path={route.path} element={<ManifestPage route={route} />} />)}</Route></Route>
    <Route element={<RouteGuard />}><Route element={<Layout />}>
      <Route path="/console" element={<Overview />} />
      <Route path="/cost" element={<CostPage />} /><Route path="/requests" element={<RequestsPage />} /><Route path="/keys" element={<KeysPage />} /><Route path="/traffic" element={<TrafficPage />} /><Route path="/alerts" element={<AlertsPage />} /><Route path="/cost/drilldown" element={<DrilldownPage />} /><Route path="/help" element={<HelpPage />} /><Route path="/live" element={<LivePage />} /><Route path="/quotas" element={<QuotasPage />} />
      <Route element={<RouteGuard requiredPermission="keys.manage" />}><Route path="/client-keys" element={<ClientKeysPage />} /><Route path="/config/pools" element={<PoolListPage />} /></Route>
      <Route element={<RouteGuard requiredPermission="audit.view" />}><Route path="/config" element={<ConfigConsolePage />} /></Route>
      <Route element={<RouteGuard requiredPermission="providers.manage" />}><Route path="/config/providers" element={<ProviderListPage />} /><Route path="/config/models" element={<ModelCatalogPage />} /></Route>
      <Route element={<RouteGuard requiredPermission="routing.edit" />}><Route path="/config/routing" element={<RoutingConfigPage />} /></Route>
      <Route path="/budgets" element={<BudgetPage />} /><Route path="/admin/users" element={<UserListPage />} /><Route path="/admin/roles" element={<RoleListPage />} /><Route path="/settings/audit" element={<AuditLogPage />} /><Route path="/settings/system" element={<SystemSettingsPage />} />
    </Route></Route>
    <Route path="/dashboard" element={<Navigate to="/console" replace />} />
  </Routes></HashRouter></LocaleProvider></QueryClientProvider>;
}
