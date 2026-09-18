import { useQuery } from '@tanstack/react-query';
import { Activity, Zap, AlertTriangle, TrendingUp, Server } from 'lucide-react';
import { fetchStatus, fetchTraffic, fetchKeys, fetchCost } from '@/lib/api';
import { formatNum } from '@/lib/utils';
import { useLocale } from '@/i18n/context';
import { ErrorState, Skeleton } from '@/components/ui';
import { AreaChart } from '@/components/charts';

export function Overview() {
  const { t } = useLocale();
  const status = useQuery({ queryKey: ['status'], queryFn: fetchStatus, refetchInterval: 10000 });
  const traffic = useQuery({ queryKey: ['traffic-overview'], queryFn: () => fetchTraffic(7) });
  const keys = useQuery({ queryKey: ['keys-overview'], queryFn: fetchKeys, refetchInterval: 30000 });

  const cost = useQuery({ queryKey: ['overview-cost'], queryFn: () => fetchCost(undefined, 24), refetchInterval: 60000 });
  const s = status.data;
  const tr = traffic.data;
  const k = keys.data;
  const cs = cost.data?.stats;

  const totalKeys = k?.pools.reduce((sum, p) => sum + p.keys.length, 0) ?? 0;
  const healthyKeys = k?.pools.reduce((sum, p) => sum + p.keys.filter(k => k.healthy).length, 0) ?? 0;

  const chartData = tr?.chart?.labels?.map((l, i) => {
    const pts1 = tr.chart.lines[0]?.points.split(' ').map(p => p.split(','));
    return {
      label: l.text,
      requests: pts1?.[i + 1] ? Math.round((1 - (parseFloat(pts1[i + 1][1]) - 40) / 160) * 100) : 0,
    };
  }) ?? [];

  const queries = [status, traffic, keys, cost];
  const isLoading = queries.some(query => query.isLoading);
  const isError = queries.some(query => query.isError);
  const retryFailed = () => {
    void Promise.all(queries.filter(query => query.isError).map(query => query.refetch()));
  };

  const statCards = [
    { label: t.overview.totalRequests, value: cs?.total_requests ?? formatNum(s?.requests_total ?? 0), icon: Activity, color: 'text-surface-800', bg: 'bg-surface-100' },
    { label: t.overview.cacheHits, value: cs ? formatNum(cs.cached_tokens) : formatNum(s?.cache_hits ?? 0), icon: Zap, color: 'text-surface-700', bg: 'bg-surface-100' },
    { label: t.overview.errors, value: cs ? formatNum(cs.total_errors) : formatNum(s?.requests_failed ?? 0), icon: AlertTriangle, color: 'text-surface-800', bg: 'bg-surface-100' },
    { label: t.overview.activeConns, value: formatNum(s?.active_connections ?? 0), icon: TrendingUp, color: 'text-surface-700', bg: 'bg-surface-100' },
    { label: t.overview.activeKeys, value: `${healthyKeys}/${totalKeys}`, icon: Server, color: 'text-surface-700', bg: 'bg-surface-100' },
  ];

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div><div className="mb-2 font-operational text-[11px] font-medium uppercase tracking-[0.18em] text-surface-400">{t.sidebar.workspace}</div><h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.overview.title}</h1><p className="mt-1.5 text-[14.5px] text-surface-600">{t.overview.subtitle}</p></div>
        <div className="flex items-center gap-2 text-xs text-surface-500"><span className="h-2 w-2 rounded-full bg-surface-900" aria-hidden="true" />{t.overview.subtitle}</div>
      </div>

      {isLoading ? <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3"><Skeleton className="h-24" /><Skeleton className="h-24" /><Skeleton className="h-24" /></div> : isError ? <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={retryFailed}>{t.common.uiRetry}</button>} /> : <>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-6">
        {statCards.map((card) => (
          <div
            key={card.label}
            className="rise group rounded-2xl border border-surface-200 bg-white p-4 shadow-[0_8px_28px_-18px_rgba(10,10,10,0.18)] transition-all duration-300 hover:-translate-y-px hover:shadow-[0_14px_34px_-20px_rgba(10,10,10,0.24)]"
          >
            <div className="flex items-start gap-3"><div className="min-w-0 flex-1"><div className="mb-1.5 text-xs font-medium text-surface-500">{card.label}</div><div className="font-operational text-2xl font-bold tracking-tight text-surface-900">{card.value}</div></div><div className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border border-surface-200 ${card.bg}`}><card.icon size={18} className={card.color} strokeWidth={1.6} /></div></div>
          </div>
        ))}
      </div>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1.35fr_0.65fr]">
        <div className="glass-card p-5">
          <div className="flex items-center justify-between mb-4">
            <h3 className="text-sm font-semibold text-surface-700">{t.overview.trafficTrend}</h3>
            <span className="badge badge-info text-[11px]">{t.overview.requests}</span>
          </div>
          <div className="h-64">
            <AreaChart
              data={chartData}
              series={[{ key: 'requests', label: t.overview.requests, color: '#0a0a0a' }]}
              height={256}
            />
          </div>
        </div>

        <div className="glass-card p-5">
          <h3 className="text-sm font-semibold text-surface-700 mb-4">{t.overview.keyHealth}</h3>
          <div className="space-y-3">
            {k?.pools.slice(0, 4).map(pool => {
              const healthy = pool.keys.filter(k => k.healthy).length;
              const pct = pool.keys.length > 0 ? (healthy / pool.keys.length) * 100 : 0;
              return (
                <div key={pool.pool_id}>
                  <div className="flex justify-between text-xs mb-1.5">
                    <span className="text-surface-600 font-medium">{pool.pool_id}</span>
                    <span className="text-surface-400">{healthy}/{pool.keys.length}</span>
                  </div>
                  <div className="h-2 overflow-hidden rounded-full bg-surface-100">
                    <div
                      style={{ width: `${pct}%` }}
                      className={`h-full rounded-full ${pct > 70 ? 'bg-surface-900' : pct > 30 ? 'bg-surface-600' : 'bg-surface-300'}`}
                    />
                  </div>
                </div>
              );
            })}
            {(!k?.pools || k.pools.length === 0) && (
              <p className="text-surface-400 text-sm py-4 text-center">{t.overview.noPools}</p>
            )}
          </div>
        </div>
      </div>

      </>}
    </div>
  );
}
