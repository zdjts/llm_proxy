import { HashRouter, Routes, Route } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LocaleProvider } from '@/i18n/context';
import { Layout } from '@/components/Layout';
import { RouteGuard } from '@/components/RouteGuard';

// ── Auth ──
import { LoginPage } from '@/pages/auth/LoginPage';

// ── Existing pages ──
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


// ── v4.1: Configuration pages (Track M) ──
import { ProviderListPage } from '@/pages/config/ProviderListPage';
import { PoolListPage } from '@/pages/config/PoolListPage';
import { RoutingConfigPage } from '@/pages/config/RoutingConfigPage';
import { ModelCatalogPage } from '@/pages/config/ModelCatalogPage';

// ── v4.1: Budget & Admin pages (Track N) ──
import { BudgetPage } from '@/pages/budget/BudgetPage';
import { UserListPage } from '@/pages/admin/UserListPage';
import { RoleListPage } from '@/pages/admin/RoleListPage';

// ── v4.1: Audit & Settings (Track P) ──
import { AuditLogPage } from '@/pages/settings/AuditLogPage';
import { SystemSettingsPage } from '@/pages/settings/SystemSettingsPage';

// ── v4.1: Stub pages for pages not yet fully built ──
function ComingSoon({ title }: { title: string }) {
  return (
    <div className="flex items-center justify-center h-64">
      <div className="text-center">
        <h2 className="text-2xl font-bold text-surface-600 mb-2">{title}</h2>
        <p className="text-surface-400">Coming soon</p>
      </div>
    </div>
  );
}

const queryClient = new QueryClient({ defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } } });

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <HashRouter>
          <Routes>
            {/* Public auth route — no Layout, no RouteGuard */}
            <Route path="/login" element={<LoginPage />} />

            {/* Protected routes with Layout */}
            <Route element={<RouteGuard />}>
              <Route element={<Layout />}>
                <Route path="/" element={<Overview />} />
                <Route path="/cost" element={<CostPage />} />
                <Route path="/requests" element={<RequestsPage />} />
                <Route path="/keys" element={<KeysPage />} />
                <Route path="/traffic" element={<TrafficPage />} />
                <Route path="/alerts" element={<AlertsPage />} />
                <Route path="/cost/drilldown" element={<DrilldownPage />} />
                <Route path="/help" element={<HelpPage />} />
                <Route path="/live" element={<LivePage />} />
                <Route element={<RouteGuard requiredPermission="keys.manage" />}>
                  <Route path="/client-keys" element={<ClientKeysPage />} />
                </Route>
                <Route path="/quotas" element={<QuotasPage />} />

                <Route element={<RouteGuard requiredPermission="audit.view" />}>
                  <Route path="/config" element={<ConfigConsolePage />} />
                </Route>
                <Route element={<RouteGuard requiredPermission="providers.manage" />}>
                  <Route path="/config/providers" element={<ProviderListPage />} />
                  <Route path="/config/models" element={<ModelCatalogPage />} />
                </Route>
                <Route element={<RouteGuard requiredPermission="keys.manage" />}>
                  <Route path="/config/pools" element={<PoolListPage />} />
                </Route>
                <Route element={<RouteGuard requiredPermission="routing.edit" />}>
                  <Route path="/config/routing" element={<RoutingConfigPage />} />
                </Route>

                {/* Budget & Admin (Track N) */}
                <Route path="/budgets" element={<BudgetPage />} />
                <Route path="/admin/users" element={<UserListPage />} />
                <Route path="/admin/roles" element={<RoleListPage />} />

                {/* Settings (Track P) */}
                <Route path="/settings/audit" element={<AuditLogPage />} />
                <Route path="/settings/system" element={<SystemSettingsPage />} />
              </Route>
            </Route>
          </Routes>
        </HashRouter>
      </LocaleProvider>
    </QueryClientProvider>
  );
}
