import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { fetchRequests } from '@/lib/api';
import type { RequestFilter } from '@/types';
import { Download } from 'lucide-react';
import { csvDownload } from '@/lib/utils';
import { useLocale } from '@/i18n/context';

export function RequestsPage() {
  const { t } = useLocale();
  const [filter, setFilter] = useState<RequestFilter>({ hours: 24 });
  const { data, refetch } = useQuery({
    queryKey: ['requests', filter],
    queryFn: () => fetchRequests(filter),
    refetchInterval: 30000,
  });

  const handleCsvExport = () => {
    if (!data?.rows) return;
    const rows = data.rows.map(r => [
      new Date(r.ts).toISOString(), r.model, r.pool_id, r.key_hash, r.status_code,
      r.prompt_tokens, r.completion_tokens, r.cached_tokens,
      r.finish_reason || '', r.error_code || '', String(r.latency_ms),
      r.ttft_ms || '', String(r.retry_count),
    ]);
    csvDownload('requests.csv', rows, [
      t.requests.thTime, t.requests.thModel, t.requests.thPool, t.requests.thKeyHash,
      t.requests.thStatus, t.requests.thPrompt, t.requests.thCompl, t.requests.thCache,
      t.requests.thFinish, t.requests.thError, t.requests.thLatency, t.requests.thTtft,
      t.requests.thRetries,
    ]);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gradient">{t.requests.title}</h1>
          <p className="text-sm text-surface-500 mt-1">
            {t.requests.subtitle.replace('{count}', String(data?.rows.length ?? 0)).replace('{hours}', String(filter.hours ?? 24))}
          </p>
        </div>
        <button onClick={handleCsvExport} className="btn-gold flex items-center gap-1.5 text-xs" disabled={!data?.rows?.length}>
          <Download size={13} /> {t.requests.csv}
        </button>
      </div>

      <div className="glass-card p-4">
        <div className="flex flex-wrap gap-2.5">
          <select value={filter.tenant ?? ''} onChange={e => setFilter(f => ({ ...f, tenant: e.target.value || undefined }))} className="input-glass">
            <option value="">{t.requests.allTenants}</option>
            {data?.tenants.map(tn => <option key={tn} value={tn}>{tn}</option>)}
          </select>
          <input placeholder={t.common.placeholder_model} value={filter.model ?? ''} onChange={e => setFilter(f => ({ ...f, model: e.target.value || undefined }))} className="input-glass w-32" />
          <input placeholder={t.common.placeholder_pool} value={filter.pool_id ?? ''} onChange={e => setFilter(f => ({ ...f, pool_id: e.target.value || undefined }))} className="input-glass w-32" />
          <input placeholder={t.common.placeholder_finish} value={filter.finish_reason ?? ''} onChange={e => setFilter(f => ({ ...f, finish_reason: e.target.value || undefined }))} className="input-glass w-32" />
          <input placeholder={t.common.placeholder_error} value={filter.error_code ?? ''} onChange={e => setFilter(f => ({ ...f, error_code: e.target.value || undefined }))} className="input-glass w-28" />
          <select value={filter.hours ?? 24} onChange={e => setFilter(f => ({ ...f, hours: parseInt(e.target.value) }))} className="input-glass">
            {[1, 6, 12, 24, 72, 168].map(h => <option key={h} value={h}>{h}h</option>)}
          </select>
          <button onClick={() => refetch()} className="btn-primary text-xs">{t.requests.filter}</button>
        </div>
      </div>

      <div className="glass-card overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-surface-100 bg-surface-50/50">
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thTime}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thModel}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thPool}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thKeyHash}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thStatus}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thPrompt}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thCompl}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thCache}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thFinish}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thError}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thLatency}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thTtft}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.requests.thRetries}</th>
              </tr>
            </thead>
            <tbody>
              {data?.rows.map((r, i) => (
                <tr key={`${r.ts}-${i}`} className="border-b border-surface-50 hover:bg-surface-50/50 transition-colors">
                  <td className="p-3 text-surface-500 text-xs whitespace-nowrap">{new Date(r.ts).toLocaleString()}</td>
                  <td className="p-3 text-surface-700 font-medium">{r.model}</td>
                  <td className="p-3 text-surface-500">{r.pool_id}</td>
                  <td className="p-3 text-surface-400 text-xs font-mono">{r.key_hash}</td>
                  <td className="p-3"><span className={`badge ${Number(r.status_code) < 300 ? 'badge-success' : 'badge-error'}`}>{r.status_code}</span></td>
                  <td className="p-3 text-right text-surface-500">{r.prompt_tokens}</td>
                  <td className="p-3 text-right text-surface-500">{r.completion_tokens}</td>
                  <td className="p-3 text-right text-surface-500">{r.cached_tokens}</td>
                  <td className="p-3 text-surface-400">{r.finish_reason || '—'}</td>
                  <td className="p-3 text-red-600">{r.error_code || '—'}</td>
                  <td className="p-3 text-right text-surface-600 tabular-nums">{r.latency_ms}ms</td>
                  <td className="p-3 text-right text-surface-400 text-xs">{r.ttft_ms || '—'}</td>
                  <td className="p-3 text-right text-surface-500">{r.retry_count}</td>
                </tr>
              ))}
              {(!data?.rows || data.rows.length === 0) && (
                <tr><td colSpan={13} className="p-10 text-center text-surface-400">{t.requests.noData}</td></tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
