export interface CostRow {
  model: string;
  pool_id: string;
  prompt_tokens: number;
  completion_tokens: number;
  cached_tokens: number;
  requests: number;
  errors: number;
  cost_usd: string;
}

export interface CostStats {
  total_requests: string;
  total_cost: string;
  avg_latency: string;
  error_rate: string;
  total_errors: number;
  prompt_tokens: number;
  completion_tokens: number;
  cached_tokens: number;
}

export interface CostResponse {
  rows: CostRow[];
  tenants: string[];
  selected_tenant: string | null;
  stats: CostStats;
}

export interface RequestRow {
  ts: number;
  model: string;
  pool_id: string;
  key_hash: string;
  status_code: string;
  prompt_tokens: string;
  completion_tokens: string;
  cached_tokens: string;
  finish_reason: string | null;
  error_code: string | null;
  latency_ms: number;
  ttft_ms: string | null;
  retry_count: number;
}

export interface RequestFilter {
  tenant?: string;
  model?: string;
  pool_id?: string;
  finish_reason?: string;
  error_code?: string;
  min_retry?: number;
  hours?: number;
}

export interface RequestsResponse {
  rows: RequestRow[];
  filter: RequestFilter;
  tenants: string[];
}

export interface KeyView {
  key_hash: string;
  weight: number;
  healthy: boolean;
  sparkline: number[];
  success_rate: string;
}

export interface PoolView {
  pool_id: string;
  keys: KeyView[];
}

export interface KeysResponse {
  pools: PoolView[];
}

export interface ChartLineData {
  points: string;
  color: string;
}

export interface ChartLabel {
  x: number;
  text: string;
}

export interface ChartData {
  lines: ChartLineData[];
  labels: ChartLabel[];
}

export interface TrafficResponse {
  chart: ChartData;
  days: number;
  w: number;
  tenants: string[];
  selected_tenant: string | null;
}

export interface AlertEvent {
  id: number;
  ts: number;
  event_type: string;
  pool_id: string;
  tenant_id: string;
  model: string;
  error_code: string;
  msg: string;
}

export interface AlertsResponse {
  events: AlertEvent[];
  event_count: number;
  selected_type: string | null;
  tenant_filter: string;
  ts_from: string;
  ts_to: string;
}

export interface DrilldownStats {
  cost: string;
  hit_rate: string;
  avg_latency: string;
}

export interface DrilldownResponse {
  model: string;
  tenant: string | null;
  stats: DrilldownStats;
  chart: ChartData;
}

export interface StatusResponse {
  uptime_secs: number;
  requests_total: number;
  requests_failed: number;
  stream_requests: number;
  cache_hits: number;
  active_connections: number;
  prompt_tokens: number;
  completion_tokens: number;
  retries: number;
  key_demotions: number;
  upstream_5xx: number;
  upstream_4xx: number;
  pools: PoolStatus[];
  alert_count: number;
}

export interface PoolStatus {
  pool_id: string;
  total_keys: number;
  healthy_keys: number;
  bad_keys: number;
}

export interface ClientKeyRecord {
  key_hash: string;
  tenant_id: string;
  created_at: number;
  enabled: boolean;
  label: string;
}

export interface ClientKeyList {
  keys: ClientKeyRecord[];
  total: number;
}

export interface QuotaSnapshot {
  tenant_id: string;
  daily_tokens_used: number;
  daily_tokens_limit: number | null;
  monthly_requests_used: number;
  monthly_requests_limit: number | null;
}

export interface LiveRequestEvent {
  event_type: string;
  request_id: string;
  model: string;
  pool_id: string;
  status_code: number;
  latency_ms: number;
  tokens: number | null;
  ts: number;
}
