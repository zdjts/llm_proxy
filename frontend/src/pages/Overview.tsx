import { useQuery } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import { Activity, Zap, AlertTriangle, TrendingUp, Server, Radio } from 'lucide-react';
import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
import { fetchStatus, fetchTraffic, fetchAlerts, fetchKeys, fetchCost } from '@/lib/api';
import { formatNum } from '@/lib/utils';
import { useLocale } from '@/i18n/context';
import { ErrorState, Skeleton } from '@/components/ui';

export function Overview() {
  const { t } = useLocale();
  const status = useQuery({ queryKey: ['status'], queryFn: fetchStatus, refetchInterval: 10000 });
  const traffic = useQuery({ queryKey: ['traffic-overview'], queryFn: () => fetchTraffic(7) });
  const alerts = useQuery({ queryKey: ['alerts-overview'], queryFn: () => fetchAlerts() });
  const keys = useQuery({ queryKey: ['keys-overview'], queryFn: fetchKeys, refetchInterval: 30000 });

  const cost = useQuery({ queryKey: ['overview-cost'], queryFn: () => fetchCost(undefined, 24), refetchInterval: 60000 });
  const s = status.data;
  const tr = traffic.data;
  const a = alerts.data;
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

  const queries = [status, traffic, alerts, keys, cost];
  const isLoading = queries.some(query => query.isLoading);
  const isError = queries.some(query => query.isError);
  const retryFailed = () => {
    void Promise.all(queries.filter(query => query.isError).map(query => query.refetch()));
  };

  const statCards = [
    { label: t.overview.totalRequests, value: cs?.total_requests ?? formatNum(s?.requests_total ?? 0), icon: Activity, color: 'text-primary-600', bg: 'bg-primary-50' },
    { label: t.overview.cacheHits, value: cs ? formatNum(cs.cached_tokens) : formatNum(s?.cache_hits ?? 0), icon: Zap, color: 'text-accent-600', bg: 'bg-accent-50' },
    { label: t.overview.errors, value: cs ? formatNum(cs.total_errors) : formatNum(s?.requests_failed ?? 0), icon: AlertTriangle, color: 'text-danger', bg: 'bg-danger/5' },
    { label: t.overview.activeConns, value: formatNum(s?.active_connections ?? 0), icon: TrendingUp, color: 'text-emerald-600', bg: 'bg-emerald-50' },
    { label: t.overview.activeKeys, value: `${healthyKeys}/${totalKeys}`, icon: Server, color: 'text-info', bg: 'bg-info/5' },
    { label: t.overview.alerts24h, value: String(a?.event_count ?? 0), icon: Radio, color: 'text-rose-600', bg: 'bg-rose-50' },
  ];

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div><div className="mb-2 text-[11px] font-medium uppercase tracking-[0.12em] text-primary-700">{t.sidebar.workspace}</div><h1 className="text-2xl font-semibold tracking-tight text-surface-900 sm:text-3xl">{t.overview.title}</h1><p className="mt-1.5 text-sm text-surface-500">{t.overview.subtitle}</p></div>
        <div className="flex items-center gap-2 text-xs text-surface-500"><span className="h-2 w-2 rounded-full bg-emerald-500" aria-hidden="true" />{t.overview.subtitle}</div>
      </div>

      {isLoading ? <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3"><Skeleton className="h-24" /><Skeleton className="h-24" /><Skeleton className="h-24" /></div> : isError ? <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={retryFailed}>{t.common.uiRetry}</button>} /> : <>
      <div className="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-surface-200 bg-surface-200 md:grid-cols-3 lg:grid-cols-6">
        {statCards.map((card, i) => (
          <motion.div
            key={card.label}
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: i * 0.05 }}
            className="group rounded-none border-0 bg-white p-4 shadow-none transition-colors hover:bg-surface-50"
          >
            <div className={`w-9 h-9 rounded-xl ${card.bg} flex items-center justify-center mb-2.5`}>
              <card.icon size={16} className={card.color} />
            </div>
            <div className="text-xl font-bold text-surface-800">{card.value}</div>
            <div className="text-[11px] text-surface-400 uppercase tracking-wider mt-0.5">{card.label}</div>
          </motion.div>
        ))}
      </div>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1.35fr_0.65fr]">
        <div className="glass-card rounded-lg p-5">
          <div className="flex items-center justify-between mb-4">
            <h3 className="text-sm font-semibold text-surface-700">{t.overview.trafficTrend}</h3>
            <span className="badge badge-purple text-[11px]">{t.overview.requests}</span>
          </div>
          <div className="h-64">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={chartData}>
                <defs>
                  <linearGradient id="reqGrad" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="#b04436" stopOpacity={0.18} />
                    <stop offset="100%" stopColor="#b04436" stopOpacity={0} />
                  </linearGradient>
                </defs>
                <CartesianGrid strokeDasharray="3 3" stroke="#e5e0d9" />
                <XAxis dataKey="label" tick={{ fill: '#8e877d', fontSize: 11 }} axisLine={false} tickLine={false} />
                <YAxis hide />
                <Tooltip
                  contentStyle={{ background: 'white', border: '1px solid #e4e7f0', borderRadius: 12, color: '#374151', boxShadow: '0 4px 20px rgba(0,0,0,0.08)' }}
                />
                <Area type="monotone" dataKey="requests" stroke="#b04436" fill="url(#reqGrad)" strokeWidth={2} />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </div>

        <div className="glass-card rounded-lg p-5">
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
                  <div className="h-2.5 rounded-full bg-surface-100 overflow-hidden">
                    <motion.div
                      initial={{ width: 0 }}
                      animate={{ width: `${pct}%` }}
                      transition={{ duration: 0.8, ease: 'easeOut' }}
                      className={`h-full rounded-full ${pct > 70 ? 'bg-emerald-500' : pct > 30 ? 'bg-amber-500' : 'bg-red-500'}`}
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

      {a?.events && a.events.length > 0 && (
        <div className="glass-card rounded-lg p-5">
          <h3 className="text-sm font-semibold text-surface-700 mb-3">{t.overview.recentAlerts}</h3>
          <div className="space-y-1">
            {a.events.slice(0, 5).map(evt => (
              <div key={evt.id} className="flex items-center gap-3 py-2.5 px-3 rounded-lg hover:bg-surface-50 transition-colors">
                <span className={`badge ${
                  evt.event_type === 'UpstreamError' ? 'badge-error' :
                  evt.event_type === 'LatencySpike' ? 'badge-warn' :
                  evt.event_type === 'RateLimited' ? 'badge-purple' : 'badge-info'
                }`}>
                  {evt.event_type}
                </span>
                <span className="text-sm text-surface-600 flex-1 truncate">{evt.msg}</span>
                <span className="text-xs text-surface-400">{new Date(evt.ts * 1000).toLocaleTimeString()}</span>
              </div>
            ))}
          </div>
        </div>
      )}
      </>}
    </div>
  );
}
