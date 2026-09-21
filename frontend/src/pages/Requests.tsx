import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useState } from 'react';
import { Download } from 'lucide-react';
import { fetchRequestDetail, fetchRequests } from '@/lib/api';
import type { RequestRow } from '@/types';
import type { LiveRequestEvent } from '@/types';
import { csvDownload } from '@/lib/utils';
import { useLocale } from '@/i18n/context';
import { EmptyState, ErrorState, Modal, Pagination, Skeleton } from '@/components/ui';

const PAGE_SIZE = 50;

function fmtTime(ts: number, locale: string): string {
  if (!ts) return '—';
  return new Date(ts).toLocaleString(locale === 'zh-CN' ? 'zh-CN' : 'en-US', {
    year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false,
  });
}

function fmtCost(n: number | null | undefined): string {
  if (n == null) return '—';
  if (n > 0 && n < 0.01) return `$${n.toFixed(4)}`;
  return `$${n.toFixed(2)}`;
}

function statusBadge(code: string) {
  const n = Number(code);
  if (n >= 200 && n < 300) return 'badge-success';
  if (n >= 400 && n < 500) return 'badge-warn';
  if (n >= 500) return 'badge-error';
  return 'badge-info';
}

function DetailValue({ value }: { value: string | number | boolean | null | undefined }) {
  if (value == null || value === '') return <span className="text-surface-400">—</span>;
  return <span className="break-all font-operational text-surface-800">{String(value)}</span>;
}

export function RequestsPage() {
  const { t, locale } = useLocale();
  const queryClient = useQueryClient();
  const [hours, setHours] = useState(0);
  const [tenant, setTenant] = useState('');
  const [model, setModel] = useState('');
  const [page, setPage] = useState(1);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const offset = (page - 1) * PAGE_SIZE;

  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['requests', hours, tenant, model, offset],
    queryFn: () => fetchRequests({
      hours,
      tenant: tenant || undefined,
      model: model || undefined,
      offset,
      limit: PAGE_SIZE,
    }),
    refetchInterval: 15000,
  });

  // Real-time refresh: every completed request is broadcast over
  // /admin/live; refetch the list the moment an event arrives so the
  // history shows the request immediately (only when viewing page 1
  // to avoid disturbing pagination).
  const [liveEvent, setLiveEvent] = useState<LiveRequestEvent | null>(null);
  useEffect(() => {
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    let ws: WebSocket | null = null;
    let closed = false;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;

    const connect = () => {
      ws = new WebSocket(`${proto}//${location.host}/admin/live`);
      ws.onmessage = (evt) => {
        try {
          const e: LiveRequestEvent = JSON.parse(evt.data);
          if (e.event_type === 'request') setLiveEvent(e);
        } catch { /* ignore malformed frames */ }
      };
      ws.onclose = () => {
        if (!closed) retryTimer = setTimeout(connect, 3000);
      };
    };
    connect();
    return () => {
      closed = true;
      if (retryTimer) clearTimeout(retryTimer);
      ws?.close();
    };
  }, []);

  useEffect(() => {
    if (liveEvent && page === 1) {
      void queryClient.invalidateQueries({ queryKey: ['requests'] });
    }
  }, [liveEvent, page, queryClient]);

  const { data: detail, isLoading: detailLoading, isError: detailError } = useQuery({
    queryKey: ['request-detail', selectedId],
    queryFn: () => fetchRequestDetail(selectedId as string),
    enabled: Boolean(selectedId),
  });

  const total = data?.total ?? 0;
  const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const from = total === 0 ? 0 : offset + 1;
  const to = Math.min(offset + (data?.rows.length ?? 0), total);

  const handleCsvExport = () => {
    if (!data?.rows.length) return;
    const rows = data.rows.map((r) => [
      r.id, fmtTime(r.ts, locale), r.model, r.pool_id, r.key_hash, r.tenant_id,
      r.status_code, r.prompt_tokens, r.completion_tokens, r.cached_tokens,
      r.finish_reason ?? '', r.error_code ?? '', String(r.latency_ms),
      r.ttft_ms ?? '', String(r.retry_count), r.is_stream ? '1' : '0',
      r.cost_usd == null ? '' : String(r.cost_usd),
    ]);
    csvDownload('requests.csv', rows, [
      t.requests.thId, t.requests.thTime, t.requests.thModel, t.requests.thPool,
      t.requests.fieldKeyHash, t.requests.fieldTenant, t.requests.thStatus,
      t.requests.fieldPrompt, t.requests.fieldCompletion, t.requests.fieldCached,
      t.requests.thFinish, t.requests.thError, t.requests.thLatency,
      t.requests.fieldTtft, t.requests.thRetry, t.requests.thStream, t.requests.thCost,
    ]);
  };

  const rangeButtons = useMemo(() => [
    [0, t.requests.allHistory],
    [24, t.requests.range24h],
    [168, t.requests.range7d],
    [720, t.requests.range30d],
  ] as const, [t]);

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div>
          <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.requests.title}</h1>
          <p className="mt-1 text-[14.5px] text-surface-600">{t.requests.subtitle}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex gap-1 rounded-lg bg-surface-100 p-0.5">
            {rangeButtons.map(([h, label]) => (
              <button key={h} type="button" onClick={() => { setHours(h); setPage(1); }}
                className={`rounded-md px-3 py-1.5 text-xs font-medium transition-all duration-200 ${
                  hours === h ? 'border border-surface-200 bg-white text-surface-900 shadow-sm' : 'text-surface-500 hover:text-surface-900'
                }`}>{label}</button>
            ))}
          </div>
          <select value={tenant} onChange={(e) => { setTenant(e.target.value); setPage(1); }} className="input-glass">
            <option value="">{t.requests.allTenants}</option>
            {data?.tenants.map((tn) => <option key={tn} value={tn}>{tn}</option>)}
          </select>
          <input value={model} onChange={(e) => { setModel(e.target.value); setPage(1); }} className="input-glass w-40" placeholder={t.common.placeholder_model} />
          <button type="button" onClick={handleCsvExport} className="btn-primary flex items-center gap-1.5 text-xs" disabled={!data?.rows?.length}>
            <Download size={13} /> {t.requests.csv}
          </button>
        </div>
      </div>

      {isLoading ? <Skeleton className="h-40 w-full" />
        : isError ? <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.common.uiRetry}</button>} />
          : !data || data.rows.length === 0 ? <EmptyState title={t.requests.noData} description={t.requests.noDataHint} />
            : <>
              <div className="flex items-center justify-between text-xs text-surface-500">
                <span>{t.requests.showing.replace('{from}', String(from)).replace('{to}', String(to)).replace('{total}', String(total))}</span>
              </div>
              <div className="glass-card overflow-hidden p-1">
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr>
                        {[t.requests.thTime, t.requests.thModel, t.requests.thPool, t.requests.thStatus, t.requests.thTokens, t.requests.thTtft, t.requests.thLatency, t.requests.thFinish, t.requests.thError, t.requests.thCost].map((header, i) => (
                          <th key={header} className={`border-b border-surface-200 p-3 font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400 ${i === 0 || i === 1 || i === 2 ? 'text-left' : 'text-right'}`}>{header}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {data.rows.map((row: RequestRow) => (
                        <tr key={row.id} className="cursor-pointer border-b border-surface-100 last:border-0 hover:bg-surface-50 transition-colors" onClick={() => setSelectedId(row.id)}>
                          <td className="whitespace-nowrap p-3 tabular-nums text-surface-600">{fmtTime(row.ts, locale)}</td>
                          <td className="p-3 font-medium text-surface-700">{row.model}</td>
                          <td className="p-3 text-surface-500">{row.pool_id}</td>
                          <td className="p-3 text-right"><span className={`badge ${statusBadge(row.status_code)}`}>{row.status_code || '—'}</span></td>
                          <td className="p-3 text-right tabular-nums text-surface-600">{row.prompt_tokens || '0'} / {row.completion_tokens || '0'} / {row.cached_tokens || '0'}</td>
                          <td className="p-3 text-right tabular-nums text-surface-600">{row.ttft_ms ?? <span className="text-surface-400">—</span>}</td>
                          <td className="p-3 text-right tabular-nums text-surface-600">{row.latency_ms}ms</td>
                          <td className="p-3 text-right text-surface-500">{row.finish_reason || '—'}</td>
                          <td className="p-3 text-right">{row.error_code ? <span className="font-medium text-danger">{row.error_code}</span> : <span className="text-surface-400">—</span>}</td>
                          <td className="p-3 text-right font-semibold tabular-nums text-surface-900">{fmtCost(row.cost_usd)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
              <Pagination page={page} pageCount={pageCount} onPageChange={setPage} />
            </>}

      <Modal open={Boolean(selectedId)} onClose={() => setSelectedId(null)} title={t.requests.detailTitle} wide>
        {detailLoading ? <Skeleton className="h-40 w-full" />
          : detailError ? <ErrorState />
            : detail ? (
              <dl className="grid max-h-[70vh] grid-cols-1 gap-x-6 gap-y-3 overflow-y-auto text-sm sm:grid-cols-2">
                {([
                  [t.requests.fieldId, detail.id],
                  [t.requests.fieldTime, fmtTime(detail.ts, locale)],
                  [t.requests.fieldModel, detail.model],
                  [t.requests.fieldPool, detail.pool_id],
                  [t.requests.fieldTenant, detail.tenant_id],
                  [t.requests.fieldKeyHash, detail.key_hash],
                  [t.requests.fieldStatus, detail.status_code],
                  [t.requests.fieldLatency, detail.latency_ms == null ? null : `${detail.latency_ms}ms`],
                  [t.requests.fieldTtft, detail.ttft_ms == null ? null : `${detail.ttft_ms}ms`],
                  [t.requests.fieldRetry, detail.retry_count],
                  [t.requests.fieldStream, detail.is_stream ? t.requests.streamYes : t.requests.streamNo],
                  [t.requests.fieldFinish, detail.finish_reason],
                  [t.requests.fieldErrorCode, detail.error_code],
                  [t.requests.fieldError, detail.error],
                  [t.requests.fieldPrompt, detail.prompt_tokens],
                  [t.requests.fieldCompletion, detail.completion_tokens],
                  [t.requests.fieldCached, detail.cached_tokens],
                  [t.requests.fieldTotal, detail.total_tokens],
                  [t.requests.fieldCost, fmtCost(detail.cost_usd)],
                  [t.requests.fieldUpstream, detail.upstream],
                  [t.requests.fieldUpstreamModel, detail.upstream_model],
                  [t.requests.fieldUserAgent, detail.user_agent],
                  [t.requests.fieldClientIp, detail.client_ip],
                  [t.requests.fieldFingerprint, detail.system_fingerprint],
                  [t.requests.fieldCacheSource, detail.cache_source],
                  [t.requests.fieldReasoning, detail.reasoning_tokens],
                  [t.requests.fieldAudio, detail.audio_tokens],
                ] as Array<[string, string | number | boolean | null | undefined]>).map(([label, value]) => (
                  <div key={label}>
                    <dt className="font-operational text-[10px] uppercase tracking-[0.12em] text-surface-400">{label}</dt>
                    <dd className="mt-0.5"><DetailValue value={value} /></dd>
                  </div>
                ))}
              </dl>
            ) : null}
      </Modal>
    </div>
  );
}
