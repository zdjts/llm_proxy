import axios from 'axios';
import type {
  CostResponse, RequestsResponse, RequestFilter, KeysResponse,
  TrafficResponse, AlertsResponse, DrilldownResponse, StatusResponse,
  ClientKeyList, ClientKeyRecord, QuotaSnapshot,
} from '@/types';

export const api = axios.create({ baseURL: '' });

let isRefreshing = false;
let failedQueue: Array<{ resolve: (t: string) => void; reject: (e: unknown) => void }> = [];

function processQueue(token: string | null, error: unknown = null) {
  failedQueue.forEach((p) => { if (token) p.resolve(token); else p.reject(error); });
  failedQueue = [];
}

api.interceptors.request.use((config) => {
  try {
    const raw = localStorage.getItem('llm-proxy-auth');
    const token = raw ? JSON.parse(raw)?.state?.accessToken : undefined;
    if (token) config.headers.Authorization = `Bearer ${token}`;
  } catch { /* authentication is handled by the server */ }
  return config;
});

api.interceptors.response.use((res) => res, async (error) => {
  const original = error.config;
  const isAuthRequest = original?.url === '/api/auth/login' || original?.url === '/api/auth/refresh';
  if (error.response?.status === 401 && original && !original._retry && !isAuthRequest) {
    if (isRefreshing) {
      return new Promise((resolve, reject) => failedQueue.push({
        resolve: (token) => { original.headers.Authorization = `Bearer ${token}`; resolve(api(original)); }, reject,
      }));
    }
    original._retry = true;
    isRefreshing = true;
    try {
      const { useAuthStore } = await import('@/stores/authStore');
      const token = await useAuthStore.getState().refresh();
      if (!token) throw error;
      processQueue(token);
      original.headers.Authorization = `Bearer ${token}`;
      return api(original);
    } catch (refreshError) {
      processQueue(null, refreshError);
      try { const { useAuthStore } = await import('@/stores/authStore'); useAuthStore.getState().logout(); } catch { /* ignore */ }
      return Promise.reject(error);
    } finally { isRefreshing = false; }
  }
  return Promise.reject(error);
});

function errorMessage(detail: unknown): string | undefined {
  if (typeof detail === 'string') return detail;
  if (detail && typeof detail === 'object') {
    const value = detail as { message?: unknown; error?: unknown; detail?: unknown };
    if (typeof value.message === 'string') return value.message;
    if (typeof value.detail === 'string') return value.detail;
    if (value.error) {
      const nested = errorMessage(value.error);
      if (nested) return nested;
    }
    try {
      const serialized = JSON.stringify(detail);
      return serialized === '{}' ? undefined : serialized;
    } catch { return undefined; }
  }
  return undefined;
}

export function apiError(error: unknown): string {
  if (axios.isAxiosError(error)) {
    const status = error.response?.status;
    const detail = error.response?.data?.error ?? error.response?.data?.message ?? error.response?.data;
    const message = errorMessage(detail);
    if (status === 401) return message || 'Authentication required.';
    if (status === 403) return 'You do not have permission for this operation.';
    if (status === 404) return 'The requested admin resource was not found.';
    if (status === 409) return 'The operation conflicts with current configuration.';
    if (status === 422) return message || 'The configuration is invalid.';
    if (status && status >= 500) return message || 'The server could not complete the operation.';
    return message || error.message || 'Request failed.';
  }
  return error instanceof Error ? error.message : 'Request failed.';
}

export async function fetchCost(tenant?: string, hours = 24) { const p = new URLSearchParams({ format: 'json', hours: String(hours) }); if (tenant) p.set('tenant', tenant); return (await api.get<CostResponse>(`/admin?${p}`)).data; }
export async function fetchRequests(filter: RequestFilter = {}) { const p = new URLSearchParams({ format: 'json' }); Object.entries(filter).forEach(([k, v]) => { if (v !== undefined) p.set(k, String(v)); }); return (await api.get<RequestsResponse>(`/admin/requests?${p}`)).data; }
export async function fetchKeys() { return (await api.get<KeysResponse>('/admin/keys?format=json')).data; }
export async function fetchTraffic(days = 7, tenant?: string) { const p = new URLSearchParams({ format: 'json', days: String(days) }); if (tenant) p.set('tenant', tenant); return (await api.get<TrafficResponse>(`/admin/traffic?${p}`)).data; }
export async function fetchAlerts(type?: string, tenant?: string) { const p = new URLSearchParams({ format: 'json' }); if (type) p.set('type', type); if (tenant) p.set('tenant', tenant); return (await api.get<AlertsResponse>(`/admin/alerts?${p}`)).data; }
export async function fetchDrilldown(model: string, tenant?: string) { const p = new URLSearchParams({ format: 'json', model }); if (tenant) p.set('tenant', tenant); return (await api.get<DrilldownResponse>(`/admin/cost/drilldown?${p}`)).data; }
export async function fetchStatus() { return (await api.get<StatusResponse>('/admin/api/status')).data; }
export async function fetchClientKeys() { return (await api.get<ClientKeyList>('/admin/api/client-keys')).data; }
export async function addClientKey(key: string, tenant: string, label: string) { return (await api.post<ClientKeyRecord>('/admin/api/client-keys', { key, tenant_id: tenant, label })).data; }
export async function updateClientKey(hash: string, updates: { enabled?: boolean; tenant_id?: string; label?: string }) { return (await api.patch<ClientKeyRecord>(`/admin/api/client-keys/${hash}`, updates)).data; }
export async function deleteClientKey(hash: string) { return (await api.delete<ClientKeyRecord>(`/admin/api/client-keys/${hash}`)).data; }
export async function rotateClientKey(hash: string, newKey: string) { return (await api.post<ClientKeyRecord>(`/admin/api/client-keys/${hash}`, { new_key: newKey })).data; }
export async function fetchQuotas() { return (await api.get<QuotaSnapshot[]>('/admin/api/quotas')).data; }

export type ConfigOverview = { version: number; providers: number; pools: number; routing: number; model_registry: number };
export type ConfigDocumentResponse = { ok: boolean; dry_run: boolean; version: number; message: string };
export type Provider = { id: string; kind: string; base_url: string; pool_id: string; api_version?: string; region?: string; metadata?: Record<string, unknown> };
export type PoolKey = { key_hash: string; weight: number };
export type Pool = { id: string; strategy: string; keys: PoolKey[] };
export type Routing = { logical_model: string; pool_id: string; default_params?: Record<string, unknown> };

export type ModelRegistry = {
  id: string; display_name: string; provider_kind: string; provider_config_id?: string | null;
  supports_vision: boolean; supports_tool_calling: boolean; supports_json_mode: boolean;
  max_context_tokens: number; max_output_tokens: number; input_price_per_1m?: number | null;
  output_price_per_1m?: number | null; capabilities_json: Record<string, unknown>; enabled: boolean;
};
export async function fetchModels() { return (await api.get<{ models: ModelRegistry[] }>('/admin/api/models')).data; }
export async function createModel(input: Omit<ModelRegistry, 'id'> & { id: string }) { return (await api.post<ModelRegistry>('/admin/api/models', input)).data; }
export async function updateModel(id: string, input: Partial<Omit<ModelRegistry, 'id'>>) { return (await api.patch<ModelRegistry>(`/admin/api/models/${encodeURIComponent(id)}`, input)).data; }

export async function fetchConfigOverview() { return (await api.get<ConfigOverview>('/admin/api/config')).data; }
export async function validateConfig(yaml: string) { return (await api.post<ConfigDocumentResponse>('/admin/api/config/validate', { yaml, dry_run: true })).data; }
export async function importConfig(yaml: string, dryRun: boolean) { return (await api.post<ConfigDocumentResponse>('/admin/api/config/import', { yaml, dry_run: dryRun })).data; }
export async function refreshConfig() { return (await api.post<{ ok: boolean; version: number }>('/admin/api/config/refresh')).data; }
export async function exportConfig() { return (await api.get('/admin/api/config/export', { responseType: 'blob' })).data as Blob; }
export async function fetchProviders() { return (await api.get<{ providers: Provider[] }>('/admin/api/providers')).data; }
export async function createProvider(input: Partial<Provider>) {
  return (await api.post('/admin/api/providers', { metadata: {}, ...input })).data;
}
export async function deleteProvider(id: string) { return (await api.delete(`/admin/api/providers/${encodeURIComponent(id)}`)).data; }
export async function fetchPools() { return (await api.get<{ pools: Pool[] }>('/admin/api/pools')).data; }
export async function createPool(input: unknown) { return (await api.post('/admin/api/pools', input)).data; }
export async function deletePool(id: string) { return (await api.delete(`/admin/api/pools/${encodeURIComponent(id)}`)).data; }
export async function fetchRouting() { return (await api.get<{ routing: Routing[] }>('/admin/api/routing')).data; }
export async function createRouting(input: unknown) { return (await api.post('/admin/api/routing', input)).data; }
export async function deleteRouting(model: string) { return (await api.delete(`/admin/api/routing/${encodeURIComponent(model)}`)).data; }
export async function downloadExport() { const blob = await exportConfig(); const url = URL.createObjectURL(blob); const a = document.createElement('a'); a.href = url; a.download = 'llm-proxy-config.yaml'; a.click(); URL.revokeObjectURL(url); }
