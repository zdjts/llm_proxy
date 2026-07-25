import axios from 'axios';
import type {
  CostResponse, RequestsResponse, RequestFilter, KeysResponse,
  TrafficResponse, AlertsResponse, DrilldownResponse, StatusResponse,
  ClientKeyList, ClientKeyRecord, QuotaSnapshot,
} from '@/types';

export const api = axios.create({ baseURL: '' });

// ── Auth interceptor (v4.1 Track K) — auto-inject JWT + refresh on 401 ──
let isRefreshing = false;
let failedQueue: Array<{ resolve: (t: string) => void; reject: (e: unknown) => void }> = [];

function processQueue(token: string | null, error: unknown = null) {
  failedQueue.forEach((p) => { if (token) p.resolve(token); else p.reject(error); });
  failedQueue = [];
}

api.interceptors.request.use((config) => {
  // Lazy import to avoid circular dependency at module init
  import('@/stores/authStore').then(m => {
    const token = m.useAuthStore.getState().getToken();
    if (token) config.headers.Authorization = `Bearer ${token}`;
  });
  // Synchronous fallback: read from localStorage directly
  try {
    const raw = localStorage.getItem('llm-proxy-auth');
    if (raw) {
      const parsed = JSON.parse(raw);
      const token = parsed?.state?.accessToken;
      if (token) config.headers.Authorization = `Bearer ${token}`;
    }
  } catch { /* ignore */ }
  return config;
});

api.interceptors.response.use(
  (res) => res,
  async (error) => {
    const original = error.config;
    if (error.response?.status === 401 && !original._retry) {
      if (isRefreshing) {
        return new Promise((resolve, reject) => {
          failedQueue.push({
            resolve: (t: string) => { original.headers.Authorization = `Bearer ${t}`; resolve(api(original)); },
            reject,
          });
        });
      }
      original._retry = true;
      isRefreshing = true;
      try {
        const { useAuthStore } = await import('@/stores/authStore');
        const newToken = await useAuthStore.getState().refresh();
        if (newToken) {
          processQueue(newToken);
          original.headers.Authorization = `Bearer ${newToken}`;
          return api(original);
        }
        processQueue(null, new Error('refresh failed'));
        useAuthStore.getState().logout();
        if (window.location.hash !== '#/login') window.location.hash = '#/login';
        return Promise.reject(error);
      } catch (e) {
        processQueue(null, e);
        try { const { useAuthStore } = await import('@/stores/authStore'); useAuthStore.getState().logout(); } catch {}
        return Promise.reject(e);
      } finally {
        isRefreshing = false;
      }
    }
    return Promise.reject(error);
  },
);

export async function fetchCost(tenant?: string, hours = 24) {
  const params = new URLSearchParams({ format: 'json', hours: String(hours) });
  if (tenant) params.set('tenant', tenant);
  const { data } = await api.get<CostResponse>(`/admin?${params}`);
  return data;
}

export async function fetchRequests(filter: RequestFilter = {}) {
  const params = new URLSearchParams({ format: 'json' });
  if (filter.tenant) params.set('tenant', filter.tenant);
  if (filter.model) params.set('model', filter.model);
  if (filter.pool_id) params.set('pool_id', filter.pool_id);
  if (filter.finish_reason) params.set('finish_reason', filter.finish_reason);
  if (filter.error_code) params.set('error_code', filter.error_code);
  if (filter.hours) params.set('hours', String(filter.hours));
  if (filter.min_retry !== undefined) params.set('min_retry', String(filter.min_retry));
  const { data } = await api.get<RequestsResponse>(`/admin/requests?${params}`);
  return data;
}

export async function fetchKeys() {
  const { data } = await api.get<KeysResponse>('/admin/keys?format=json');
  return data;
}

export async function fetchTraffic(days = 7, tenant?: string) {
  const params = new URLSearchParams({ format: 'json', days: String(days) });
  if (tenant) params.set('tenant', tenant);
  const { data } = await api.get<TrafficResponse>(`/admin/traffic?${params}`);
  return data;
}

export async function fetchAlerts(type?: string, tenant?: string) {
  const params = new URLSearchParams({ format: 'json' });
  if (type) params.set('type', type);
  if (tenant) params.set('tenant', tenant);
  const { data } = await api.get<AlertsResponse>(`/admin/alerts?${params}`);
  return data;
}

export async function fetchDrilldown(model: string, tenant?: string) {
  const params = new URLSearchParams({ format: 'json', model });
  if (tenant) params.set('tenant', tenant);
  const { data } = await api.get<DrilldownResponse>(`/admin/cost/drilldown?${params}`);
  return data;
}

export async function fetchStatus() {
  const { data } = await api.get<StatusResponse>('/admin/api/status');
  return data;
}

export async function fetchClientKeys(): Promise<ClientKeyList> {
  const { data } = await api.get<ClientKeyList>('/admin/api/client-keys');
  return data;
}

export async function addClientKey(key: string, tenant: string, label: string) {
  const { data } = await api.post<ClientKeyRecord>('/admin/api/client-keys', { key, tenant_id: tenant, label });
  return data;
}

export async function updateClientKey(keyHash: string, updates: { enabled?: boolean; tenant_id?: string; label?: string }) {
  const { data } = await api.patch<ClientKeyRecord>(`/admin/api/client-keys/${keyHash}`, updates);
  return data;
}

export async function deleteClientKey(keyHash: string) {
  const { data } = await api.delete<ClientKeyRecord>(`/admin/api/client-keys/${keyHash}`);
  return data;
}

export async function rotateClientKey(keyHash: string, newKey: string) {
  const { data } = await api.post<ClientKeyRecord>(`/admin/api/client-keys/${keyHash}`, { new_key: newKey });
  return data;
}

export async function fetchQuotas() {
  const { data } = await api.get<QuotaSnapshot[]>('/admin/api/quotas');
  return data;
}
