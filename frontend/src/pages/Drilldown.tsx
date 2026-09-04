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
        <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{model || t.drilldown.unknownModel}</h1>
        <p className="mt-1 text-[14.5px] text-surface-600">
          {t.drilldown.subtitle}
          {tenant ? <span className="badge badge-info ml-2">Tenant: {tenant}</span> : ''}
        </p>
      </div>

      {isLoading && <Skeleton className="h-32 w-full" />}
      {isError && <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.common.uiRetry}</button>} />}
      {!isLoading && !isError && data?.stats && (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
          {[[t.drilldown.estCost, data.stats.cost], [t.drilldown.cacheHitRate, data.stats.hit_rate], [t.drilldown.avgLatency, data.stats.avg_latency]].map(([label, value]) => <div key={String(label)} className="glass-card p-4"><div className="text-xs font-medium text-surface-500">{label}</div><div className="mt-1 font-operational text-2xl font-bold tracking-tight text-surface-900">{value}</div></div>)}
        </div>
      )}

      {!isLoading && !isError && chartData.length > 0 && (
        <div className="glass-card p-5">
          <h3 className="text-sm font-semibold text-surface-700 mb-4">{t.drilldown.hourlyBreakdown}</h3>
          <div className="h-80">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={chartData}>
                <defs>
                  <linearGradient id="dr1" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#0a0a0a" stopOpacity={0.12} /><stop offset="100%" stopColor="#0a0a0a" stopOpacity={0} /></linearGradient>
                  <linearGradient id="dr2" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#525252" stopOpacity={0.15} /><stop offset="100%" stopColor="#525252" stopOpacity={0} /></linearGradient>
                  <linearGradient id="dr3" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#a3a3a3" stopOpacity={0.15} /><stop offset="100%" stopColor="#a3a3a3" stopOpacity={0} /></linearGradient>
                </defs>
                <CartesianGrid strokeDasharray="3 3" stroke="#e5e0d9" />
                <XAxis dataKey="hour" tick={{ fill: '#8e877d', fontSize: 10 }} axisLine={false} tickLine={false} />
                <YAxis hide />
                <Tooltip contentStyle={{ background: 'white', border: '1px solid #e4e7f0', borderRadius: 12, color: '#374151', boxShadow: '0 4px 20px rgba(0,0,0,0.08)' }} />
                <Area type="monotone" dataKey="requests" stroke="#0a0a0a" fill="url(#dr1)" strokeWidth={1.5} />
                <Area type="monotone" dataKey="promptTokens" stroke="#525252" fill="url(#dr2)" strokeWidth={1.5} />
                <Area type="monotone" dataKey="completionTokens" stroke="#a3a3a3" fill="url(#dr3)" strokeWidth={1.5} />
              </AreaChart>
            </ResponsiveContainer>
          </div>
          <div className="flex gap-5 mt-3 px-2 text-xs text-surface-400">
            <div className="flex items-center gap-1.5"><div className="h-3 w-3 rounded-sm bg-surface-900" /> {t.drilldown.requests}</div>
            <div className="flex items-center gap-1.5"><div className="h-3 w-3 rounded-sm bg-surface-600" /> {t.drilldown.promptTokens}</div>
            <div className="flex items-center gap-1.5"><div className="h-3 w-3 rounded-sm bg-surface-400" /> {t.drilldown.completionTokens}</div>
          </div>
        </div>
      )}
      {!isLoading && !isError && chartData.length === 0 && <EmptyState title={t.common.uiNoData} />}
    </div>
  );
}
