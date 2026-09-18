import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { fetchCost } from '@/lib/api';
import { Download } from 'lucide-react';
import { csvDownload } from '@/lib/utils';
import { useLocale } from '@/i18n/context';
import { EmptyState, ErrorState, Skeleton } from '@/components/ui';

export function CostPage() {
  const { t } = useLocale();
  const [tenant, setTenant] = useState('');
  const [hours, setHours] = useState(24);
  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['cost', tenant, hours],
    queryFn: () => fetchCost(tenant || undefined, hours),
    refetchInterval: 60000,
  });

  const handleCsvExport = () => {
    if (!data?.rows) return;
    const rows = data.rows.map(r => [
      r.model, r.pool_id, String(r.prompt_tokens), String(r.completion_tokens),
      String(r.cached_tokens), String(r.requests), String(r.errors), r.cost_usd,
    ]);
    csvDownload('cost-overview.csv', rows, [
      t.cost.thModel, t.cost.thPool, t.cost.thPrompt, t.cost.thCompletion,
      t.cost.thCacheHits, t.cost.thRequests, t.cost.thErrors, t.cost.thCost,
    ]);
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div>
          <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.cost.title}</h1>
          <p className="mt-1 text-[14.5px] text-surface-600">{t.cost.subtitle}</p>
        </div>
        <div className="flex gap-2">
          <select value={hours} onChange={e => setHours(Number(e.target.value))} className="input-glass">
            {[24, 72, 168, 720].map(h => <option key={h} value={h}>{h >= 168 ? `${h / 24}d` : `${h}h`}</option>)}
          </select>
          <select value={tenant} onChange={e => setTenant(e.target.value)} className="input-glass">
            <option value="">{t.cost.allTenants}</option>
            {data?.tenants.map(tn => <option key={tn} value={tn}>{tn}</option>)}
          </select>
          <button onClick={handleCsvExport} className="btn-primary flex items-center gap-1.5 text-xs" disabled={!data?.rows?.length}>
            <Download size={13} /> {t.cost.csv}
          </button>
        </div>
      </div>

      {!isLoading && !isError && data?.stats && (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          {[
            { label: t.cost.requests24h, value: data.stats.total_requests, key: 'req' },
            { label: t.cost.estCost, value: data.stats.total_cost, key: 'cost' },
            { label: t.cost.avgLatency, value: data.stats.avg_latency, key: 'lat' },
            { label: t.cost.errorRate, value: data.stats.error_rate, key: 'err' },
            { label: t.cost.totalErrors, value: data.stats.total_errors.toLocaleString(), key: 'errors' },
            { label: t.cost.promptTokens, value: data.stats.prompt_tokens.toLocaleString(), key: 'prompt' },
            { label: t.cost.completionTokens, value: data.stats.completion_tokens.toLocaleString(), key: 'completion' },
            { label: t.cost.cachedTokens, value: data.stats.cached_tokens.toLocaleString(), key: 'cached' },
          ].map((s, i) => (
            <div key={s.key} style={{ animationDelay: `${Math.min(i, 11) * 50}ms` }} className="rise rounded-2xl border border-surface-200 bg-white p-4 shadow-[0_8px_28px_-18px_rgba(10,10,10,0.18)] transition-shadow duration-300 hover:shadow-[0_14px_34px_-20px_rgba(10,10,10,0.24)]">
              <div className="text-xs font-medium text-surface-500">{s.label}</div>
              <div className="mt-1 font-operational text-2xl font-bold tracking-tight text-surface-900">{s.value}</div>
            </div>
          ))}
        </div>
      )}

      {isLoading ? <Skeleton className="h-32 w-full" /> : isError ? <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.common.uiRetry}</button>} /> : <div className="glass-card overflow-hidden p-1">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr>
                <th className="text-left border-b border-surface-200 p-3 font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{t.cost.thModel}</th>
                <th className="text-left border-b border-surface-200 p-3 font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{t.cost.thPool}</th>
                <th className="text-right border-b border-surface-200 p-3 font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{t.cost.thPrompt}</th>
                <th className="text-right border-b border-surface-200 p-3 font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{t.cost.thCompletion}</th>
                <th className="text-right border-b border-surface-200 p-3 font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{t.cost.thCacheHits}</th>
                <th className="text-right border-b border-surface-200 p-3 font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{t.cost.thRequests}</th>
                <th className="text-right border-b border-surface-200 p-3 font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{t.cost.thErrors}</th>
                <th className="text-right border-b border-surface-200 p-3 font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{t.cost.thCost}</th>
              </tr>
            </thead>
            <tbody>
              {data?.rows.map((r, i) => (
                <tr key={`${r.model}-${r.pool_id}-${i}`} className="border-b border-surface-100 last:border-0 hover:bg-surface-50 transition-colors">
                  <td className="p-3 text-surface-700 font-medium">{r.model}</td>
                  <td className="p-3 text-surface-500">{r.pool_id}</td>
                  <td className="p-3 text-right text-surface-600 tabular-nums">{r.prompt_tokens.toLocaleString()}</td>
                  <td className="p-3 text-right text-surface-600 tabular-nums">{r.completion_tokens.toLocaleString()}</td>
                  <td className="p-3 text-right text-surface-500">{r.cached_tokens.toLocaleString()}</td>
                  <td className="p-3 text-right text-surface-600">{r.requests.toLocaleString()}</td>
                  <td className="p-3 text-right">{r.errors > 0 ? <span className="text-red-600 font-medium">{r.errors}</span> : <span className="text-surface-400">0</span>}</td>
                  <td className="p-3 text-right font-semibold text-surface-900 tabular-nums">{r.cost_usd}</td>
                </tr>
              ))}
              {(!data?.rows || data.rows.length === 0) && (
                <tr><td colSpan={8} className="p-10"><EmptyState title={t.cost.noData} /></td></tr>
              )}
            </tbody>
          </table>
        </div>
      </div>}
    </div>
  );
}
