import { useQuery } from '@tanstack/react-query';
import { useSearchParams, Link } from 'react-router-dom';
import { fetchDrilldown } from '@/lib/api';
import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
import { ArrowLeft } from 'lucide-react';
import { useLocale } from '@/i18n/context';
import { EmptyState, ErrorState, Skeleton } from '@/components/ui';

export function DrilldownPage() {
  const { t } = useLocale();
  const [params] = useSearchParams();
  const model = params.get('model') || '';
  const tenant = params.get('tenant') || undefined;

  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['drilldown', model, tenant],
    queryFn: () => fetchDrilldown(model, tenant),
    enabled: !!model,
  });

  const chartData = data?.chart?.labels?.map((l, i) => {
    const pts1 = data.chart.lines[0]?.points.split(' ').map(p => p.split(','));
    const pts2 = data.chart.lines[1]?.points.split(' ').map(p => p.split(','));
    const pts3 = data.chart.lines[2]?.points.split(' ').map(p => p.split(','));
    return {
      hour: l.text,
      requests: pts1?.[i + 1] ? Math.round((1 - (parseFloat(pts1[i + 1][1]) - 40) / 160) * 100) : 0,
      promptTokens: pts2?.[i + 1] ? Math.round((1 - (parseFloat(pts2[i + 1][1]) - 40) / 160) * 100) : 0,
      completionTokens: pts3?.[i + 1] ? Math.round((1 - (parseFloat(pts3[i + 1][1]) - 40) / 160) * 100) : 0,
    };
  }) ?? [];

  return (
    <div className="space-y-6">
      <div className="border-b border-surface-200 pb-5"><Link to="/cost" className="inline-flex items-center gap-1.5 text-sm text-surface-400 hover:text-surface-600 transition-colors">
        <ArrowLeft size={14} /> {t.drilldown.back}
      </Link></div>

      <div>
        <h1 className="text-xl font-semibold text-surface-900 sm:text-2xl">{model || t.drilldown.unknownModel}</h1>
        <p className="text-sm text-surface-500 mt-1">
          {t.drilldown.subtitle}
          {tenant ? <span className="badge badge-info ml-2">Tenant: {tenant}</span> : ''}
        </p>
      </div>

      {isLoading && <Skeleton className="h-32 w-full" />}
      {isError && <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.common.uiRetry}</button>} />}
      {!isLoading && !isError && data?.stats && (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <div className="glass-card rounded-lg p-4"><div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.drilldown.estCost}</div><div className="text-2xl font-bold text-accent-600 mt-1">{data.stats.cost}</div></div>
          <div className="glass-card p-4"><div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.drilldown.cacheHitRate}</div><div className="text-2xl font-bold text-emerald-600 mt-1">{data.stats.hit_rate}</div></div>
          <div className="glass-card p-4"><div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.drilldown.avgLatency}</div><div className="text-2xl font-bold text-info mt-1">{data.stats.avg_latency}</div></div>
        </div>
      )}

      {!isLoading && !isError && chartData.length > 0 && (
        <div className="glass-card rounded-lg p-5">
          <h3 className="text-sm font-semibold text-surface-700 mb-4">{t.drilldown.hourlyBreakdown}</h3>
          <div className="h-80">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={chartData}>
                <defs>
                  <linearGradient id="dr1" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#b04436" stopOpacity={0.18} /><stop offset="100%" stopColor="#b04436" stopOpacity={0} /></linearGradient>
                  <linearGradient id="dr2" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#4f7887" stopOpacity={0.18} /><stop offset="100%" stopColor="#4f7887" stopOpacity={0} /></linearGradient>
                  <linearGradient id="dr3" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#a98537" stopOpacity={0.18} /><stop offset="100%" stopColor="#a98537" stopOpacity={0} /></linearGradient>
                </defs>
                <CartesianGrid strokeDasharray="3 3" stroke="#e5e0d9" />
                <XAxis dataKey="hour" tick={{ fill: '#8e877d', fontSize: 10 }} axisLine={false} tickLine={false} />
                <YAxis hide />
                <Tooltip contentStyle={{ background: 'white', border: '1px solid #e4e7f0', borderRadius: 12, color: '#374151', boxShadow: '0 4px 20px rgba(0,0,0,0.08)' }} />
                <Area type="monotone" dataKey="requests" stroke="#b04436" fill="url(#dr1)" strokeWidth={1.5} />
                <Area type="monotone" dataKey="promptTokens" stroke="#4f7887" fill="url(#dr2)" strokeWidth={1.5} />
                <Area type="monotone" dataKey="completionTokens" stroke="#a98537" fill="url(#dr3)" strokeWidth={1.5} />
              </AreaChart>
            </ResponsiveContainer>
          </div>
          <div className="flex gap-5 mt-3 px-2 text-xs text-surface-400">
            <div className="flex items-center gap-1.5"><div className="w-3 h-3 rounded-sm bg-blue-500" /> {t.drilldown.requests}</div>
            <div className="flex items-center gap-1.5"><div className="w-3 h-3 rounded-sm bg-red-500" /> {t.drilldown.promptTokens}</div>
            <div className="flex items-center gap-1.5"><div className="w-3 h-3 rounded-sm bg-emerald-500" /> {t.drilldown.completionTokens}</div>
          </div>
        </div>
      )}
      {!isLoading && !isError && chartData.length === 0 && <EmptyState title={t.common.uiNoData} />}
    </div>
  );
}
