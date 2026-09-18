import type {
  CostResponse, RequestsResponse, RequestFilter, RequestDetail, KeysResponse,
  TrafficResponse, AlertsResponse, DrilldownResponse, StatusResponse,
  ClientKeyList, ClientKeyRecord,
} from '@/types';

export class ApiError extends Error {
  constructor(message: string, readonly status?: number) {
    super(message);
    this.name = 'ApiError';
  }
}

function detailMessage(detail: unknown): string | undefined {
  if (typeof detail === 'string') return detail;
  if (detail && typeof detail === 'object') {
    const value = detail as { message?: unknown; error?: unknown; detail?: unknown };
    if (typeof value.message === 'string') return value.message;
    if (typeof value.detail === 'string') return value.detail;
    if (value.error) {
      const nested = detailMessage(value.error);
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
  if (error instanceof ApiError) {
    const { status, message } = error;
    if (status === 401) return message === 'Request failed.' ? 'Authentication required.' : message;
    if (status === 403) return 'You do not have permission for this operation.';
    if (status === 404) return 'The requested admin resource was not found.';
    if (status === 409) return 'The operation conflicts with current configuration.';
    if (status === 422) return message === 'Request failed.' ? 'The configuration is invalid.' : message;
    if (status && status >= 500) return message === 'Request failed.' ? 'The server could not complete the operation.' : message;
    return message;
  }
  return error instanceof Error ? error.message : 'Request failed.';
}

async function request<T>(method: string, url: string, body?: unknown, expectBlob = false): Promise<T> {
  const response = await fetch(url, {
    method,
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!response.ok) {
    let detail: unknown = undefined;
    try { detail = await response.json(); } catch { /* body absent or non-JSON */ }
    const payload = detail as { error?: unknown; message?: unknown } | undefined;
    const raw = payload?.error ?? payload?.message ?? payload;
    const message = detailMessage(raw) ?? response.statusText ?? 'Request failed.';
    throw new ApiError(message, response.status);
  }
  if (expectBlob) return (await response.blob()) as T;
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

const get = <T,>(url: string) => request<T>('GET', url);
const post = <T,>(url: string, body?: unknown) => request<T>('POST', url, body);
const patch = <T,>(url: string, body: unknown) => request<T>('PATCH', url, body);
const del = <T,>(url: string) => request<T>('DELETE', url);

export async function fetchCost(tenant?: string, hours = 24) { const p = new URLSearchParams({ format: 'json', hours: String(hours) }); if (tenant) p.set('tenant', tenant); return get<CostResponse>(`/admin?${p}`); }
export async function fetchRequests(filter: RequestFilter = {}) {
  const p = new URLSearchParams({ format: 'json' });
  if (filter.hours !== undefined) p.set('hours', String(filter.hours));
  if (filter.offset !== undefined) p.set('offset', String(filter.offset));
  if (filter.limit !== undefined) p.set('limit', String(filter.limit));
  if (filter.tenant) p.set('tenant', filter.tenant);
  if (filter.model) p.set('model', filter.model);
  if (filter.pool_id) p.set('pool_id', filter.pool_id);
  if (filter.finish_reason) p.set('finish_reason', filter.finish_reason);
  if (filter.error_code) p.set('error_code', filter.error_code);
  if (filter.min_retry !== undefined) p.set('min_retry', String(filter.min_retry));
  return get<RequestsResponse>(`/admin/requests?${p}`);
}
export async function fetchRequestDetail(id: string) {
  return get<RequestDetail>(`/admin/requests/${encodeURIComponent(id)}`);
}
export async function fetchKeys() { return get<KeysResponse>('/admin/keys?format=json'); }
export async function fetchTraffic(days = 7, tenant?: string) { const p = new URLSearchParams({ format: 'json', days: String(days) }); if (tenant) p.set('tenant', tenant); return get<TrafficResponse>(`/admin/traffic?${p}`); }
export async function fetchDrilldown(model: string, tenant?: string) { const p = new URLSearchParams({ format: 'json', model }); if (tenant) p.set('tenant', tenant); return get<DrilldownResponse>(`/admin/cost/drilldown?${p}`); }
export async function fetchStatus() { return get<StatusResponse>('/admin/api/status'); }
export async function fetchClientKeys() { return get<ClientKeyList>('/admin/api/client-keys'); }
export async function addClientKey(key: string, tenant: string, label: string) { return post<ClientKeyRecord>('/admin/api/client-keys', { key, tenant_id: tenant, label }); }
export async function updateClientKey(hash: string, updates: { enabled?: boolean; tenant_id?: string; label?: string }) { return patch<ClientKeyRecord>(`/admin/api/client-keys/${hash}`, updates); }
export async function deleteClientKey(hash: string) { return del<ClientKeyRecord>(`/admin/api/client-keys/${hash}`); }
export async function rotateClientKey(hash: string, newKey: string) { return post<ClientKeyRecord>(`/admin/api/client-keys/${hash}`, { new_key: newKey }); }

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
export async function fetchModels() { return get<{ models: ModelRegistry[] }>('/admin/api/models'); }
export async function createModel(input: Omit<ModelRegistry, 'id'> & { id: string }) { return post<ModelRegistry>('/admin/api/models', input); }
export async function updateModel(id: string, input: Partial<Omit<ModelRegistry, 'id'>>) { return patch<ModelRegistry>(`/admin/api/models/${encodeURIComponent(id)}`, input); }

export async function fetchConfigOverview() { return get<ConfigOverview>('/admin/api/config'); }
export async function validateConfig(yaml: string) { return post<ConfigDocumentResponse>('/admin/api/config/validate', { yaml, dry_run: true }); }
export async function importConfig(yaml: string, dryRun: boolean) { return post<ConfigDocumentResponse>('/admin/api/config/import', { yaml, dry_run: dryRun }); }
export async function refreshConfig() { return post<{ ok: boolean; version: number }>('/admin/api/config/refresh'); }
export async function exportConfig() { return request<Blob>('GET', '/admin/api/config/export', undefined, true); }
export async function fetchProviders() { return get<{ providers: Provider[] }>('/admin/api/providers'); }
export async function createProvider(input: Partial<Provider>) { return post<Provider>('/admin/api/providers', { metadata: {}, ...input }); }
export async function deleteProvider(id: string) { return del<void>(`/admin/api/providers/${encodeURIComponent(id)}`); }
export async function fetchPools() { return get<{ pools: Pool[] }>('/admin/api/pools'); }
export async function createPool(input: unknown) { return post<void>('/admin/api/pools', input); }
export async function deletePool(id: string) { return del<void>(`/admin/api/pools/${encodeURIComponent(id)}`); }
export async function fetchRouting() { return get<{ routing: Routing[] }>('/admin/api/routing'); }
export async function createRouting(input: unknown) { return post<void>('/admin/api/routing', input); }
export async function deleteRouting(model: string) { return del<void>(`/admin/api/routing/${encodeURIComponent(model)}`); }
export async function downloadExport() { const blob = await exportConfig(); const url = URL.createObjectURL(blob); const a = document.createElement('a'); a.href = url; a.download = 'llm-proxy-config.yaml'; a.click(); URL.revokeObjectURL(url); }

// ── Usage analytics ──
export type UsageSummary = { requests: number; errors: number; prompt_tokens: number; completion_tokens: number; cached_tokens: number; avg_latency_ms: number; cost_usd: number };
export type UsagePoint = UsageSummary & { ts: number; label: string };
export type UsageModelRow = { model: string; requests: number; errors: number; prompt_tokens: number; completion_tokens: number; cached_tokens: number; avg_latency_ms: number; cost_usd: number; cache_hit_rate: number };
export type UsageResponse = { hours: number; today: UsageSummary; total: UsageSummary; trend: UsagePoint[]; models: UsageModelRow[]; tenants: string[]; selected_tenant: string | null };
export async function fetchUsage(hours = 168, tenant?: string) { const p = new URLSearchParams({ hours: String(hours) }); if (tenant) p.set('tenant', tenant); return get<UsageResponse>(`/admin/api/usage?${p}`); }
