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
  id: string;
  ts: number;
  model: string;
  pool_id: string;
  key_hash: string;
  tenant_id: string;
  status_code: string;
  prompt_tokens: string;
  completion_tokens: string;
  cached_tokens: string;
  finish_reason: string | null;
  error_code: string | null;
  latency_ms: number;
  ttft_ms: string | null;
  retry_count: number;
  is_stream: boolean;
  cost_usd: number | null;
}

export interface RequestFilter {
  tenant?: string;
  model?: string;
  pool_id?: string;
  finish_reason?: string;
  error_code?: string;
  min_retry?: number;
  hours?: number;
  offset?: number;
  limit?: number;
}

export interface RequestsResponse {
  rows: RequestRow[];
  filter: RequestFilter;
  tenants: string[];
  total: number;
  has_more: boolean;
}

export interface RequestDetail {
  id: string;
  ts: number;
  client_ip: string | null;
  model: string;
  pool_id: string;
  key_hash: string;
  upstream: string | null;
  status_code: number | null;
  latency_ms: number | null;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  total_tokens: number | null;
  is_stream: boolean;
  error: string | null;
  cached_tokens: number | null;
  cache_creation_tokens: number | null;
  cache_source: string | null;
  reasoning_tokens: number | null;
  audio_tokens: number | null;
  ttft_ms: number | null;
  upstream_model: string | null;
  system_fingerprint: string | null;
  finish_reason: string | null;
  error_code: string | null;
  retry_count: number;
  tenant_id: string;
  user_agent: string | null;
  cost_usd: number | null;
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
