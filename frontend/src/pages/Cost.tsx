import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { motion } from 'framer-motion';
import { fetchCost } from '@/lib/api';
import { Download } from 'lucide-react';
import { csvDownload } from '@/lib/utils';
import { useLocale } from '@/i18n/context';

export function CostPage() {
  const { t } = useLocale();
  const [tenant, setTenant] = useState('');
  const [hours, setHours] = useState(24);
  const { data } = useQuery({
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
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gradient">{t.cost.title}</h1>
          <p className="text-sm text-surface-500 mt-1">{t.cost.subtitle}</p>
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

      {data?.stats && (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
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
            <motion.div key={s.key} initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: i * 0.06 }} className="stat-card">
              <div className="text-[11px] text-surface-400 uppercase tracking-wider">{s.label}</div>
              <div className="text-2xl font-bold text-surface-800 mt-1">{s.value}</div>
            </motion.div>
          ))}
        </div>
      )}

      <div className="glass-card overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-surface-100 bg-surface-50/50">
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.cost.thModel}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.cost.thPool}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.cost.thPrompt}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.cost.thCompletion}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.cost.thCacheHits}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.cost.thRequests}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.cost.thErrors}</th>
                <th className="text-right p-3 text-xs text-surface-400 uppercase font-semibold">{t.cost.thCost}</th>
              </tr>
            </thead>
            <tbody>
              {data?.rows.map((r, i) => (
                <tr key={`${r.model}-${r.pool_id}-${i}`} className="border-b border-surface-50 hover:bg-surface-50/50 transition-colors">
                  <td className="p-3 text-surface-700 font-medium">{r.model}</td>
                  <td className="p-3 text-surface-500">{r.pool_id}</td>
                  <td className="p-3 text-right text-surface-600 tabular-nums">{r.prompt_tokens.toLocaleString()}</td>
                  <td className="p-3 text-right text-surface-600 tabular-nums">{r.completion_tokens.toLocaleString()}</td>
                  <td className="p-3 text-right text-surface-500">{r.cached_tokens.toLocaleString()}</td>
                  <td className="p-3 text-right text-surface-600">{r.requests.toLocaleString()}</td>
                  <td className="p-3 text-right">{r.errors > 0 ? <span className="text-red-600 font-medium">{r.errors}</span> : <span className="text-surface-400">0</span>}</td>
                  <td className="p-3 text-right text-accent-600 font-semibold tabular-nums">{r.cost_usd}</td>
                </tr>
              ))}
              {(!data?.rows || data.rows.length === 0) && (
                <tr><td colSpan={8} className="p-10 text-center text-surface-400">{t.cost.noData}</td></tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
