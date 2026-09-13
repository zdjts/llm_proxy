import { HashRouter, Navigate, Route, Routes } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LocaleProvider } from '@/i18n/context';
import { Layout } from '@/components/Layout';
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
import { ConfigConsolePage } from '@/pages/ConfigConsole';
import { ProviderListPage } from '@/pages/config/ProviderListPage';
import { PoolListPage } from '@/pages/config/PoolListPage';
import { RoutingConfigPage } from '@/pages/config/RoutingConfigPage';
import { ModelCatalogPage } from '@/pages/config/ModelCatalogPage';
import { SystemSettingsPage } from '@/pages/settings/SystemSettingsPage';

const queryClient = new QueryClient({ defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } } });

export default function App() {
  return <QueryClientProvider client={queryClient}><LocaleProvider><HashRouter><Routes>
    <Route element={<Layout />}>
      <Route path="/" element={<Navigate to="/console" replace />} />
      <Route path="/console" element={<Overview />} />
      <Route path="/cost" element={<CostPage />} />
      <Route path="/requests" element={<RequestsPage />} />
      <Route path="/keys" element={<KeysPage />} />
      <Route path="/traffic" element={<TrafficPage />} />
      <Route path="/alerts" element={<AlertsPage />} />
      <Route path="/cost/drilldown" element={<DrilldownPage />} />
      <Route path="/help" element={<HelpPage />} />
      <Route path="/live" element={<LivePage />} />
      <Route path="/client-keys" element={<ClientKeysPage />} />
      <Route path="/config" element={<ConfigConsolePage />} />
      <Route path="/config/pools" element={<PoolListPage />} />
      <Route path="/config/providers" element={<ProviderListPage />} />
      <Route path="/config/models" element={<ModelCatalogPage />} />
      <Route path="/config/routing" element={<RoutingConfigPage />} />
      <Route path="/settings/system" element={<SystemSettingsPage />} />
    </Route>
    <Route path="/dashboard" element={<Navigate to="/console" replace />} />
    <Route path="*" element={<Navigate to="/console" replace />} />
  </Routes></HashRouter></LocaleProvider></QueryClientProvider>;
}
