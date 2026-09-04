import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { fetchTraffic } from '@/lib/api';
import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, BarChart, Bar } from 'recharts';
import { Download } from 'lucide-react';
import { csvDownload } from '@/lib/utils';
import { useLocale } from '@/i18n/context';
import { EmptyState, ErrorState, Skeleton } from '@/components/ui';

export function TrafficPage() {
  const { t } = useLocale();
  const [days, setDays] = useState(7);
  const [tenant, setTenant] = useState('');
  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['traffic', days, tenant],
    queryFn: () => fetchTraffic(days, tenant || undefined),
    refetchInterval: 60000,
  });

  const hasTrafficData = Boolean(data?.chart.labels.length && data.chart.lines.some(line => line.points.trim()));
  const chartData = data?.chart?.labels?.map((l, i) => {
    const pts1 = data.chart.lines[0]?.points.split(' ').map(p => p.split(','));
    const pts2 = data.chart.lines[1]?.points.split(' ').map(p => p.split(','));
    return {
      label: l.text,
      requests: pts1?.[i + 1] ? Math.round((1 - (parseFloat(pts1[i + 1][1]) - 40) / 160) * 100) : 0,
      latencyMs: pts2?.[i + 1] ? Math.round((1 - (parseFloat(pts2[i + 1][1]) - 40) / 160) * 100) : 0,
    };
  }) ?? [];

  const totalRequests = chartData.reduce((s, d) => s + d.requests, 0);
  const avgLatency = chartData.length > 0 ? Math.round(chartData.reduce((s, d) => s + d.latencyMs, 0) / chartData.length) : 0;

  const handleCsvExport = () => {
    const rows = chartData.map(d => [d.label, String(d.requests), String(d.latencyMs)]);
    csvDownload('traffic.csv', rows, ['Time', t.traffic.requests, t.traffic.latency]);
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div>
          <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.traffic.title}</h1>
          <p className="mt-1 text-[14.5px] text-surface-600">{t.traffic.subtitle.replace('{days}', String(days))}</p>
        </div>
        <button onClick={handleCsvExport} className="btn-primary flex items-center gap-1.5 text-xs" disabled={chartData.length === 0}>
          <Download size={13} /> {t.traffic.csv}
        </button>
      </div>

      <div className="glass-card flex flex-wrap items-center gap-3 p-4" role="search">
        <div className="flex gap-1 p-0.5 rounded-lg bg-surface-100">
          {[1, 7, 14, 30].map(d => (
            <button key={d} onClick={() => setDays(d)}
              className={`px-3 py-1.5 rounded-md text-xs font-medium transition-all duration-200 ${
                days === d ? 'border border-surface-200 bg-white text-surface-900 shadow-sm' : 'text-surface-500 hover:text-surface-900'
              }`}>{d}d</button>
          ))}
        </div>
        <select value={tenant} onChange={e => setTenant(e.target.value)} className="input-glass">
          <option value="">{t.traffic.allTenants}</option>
          {data?.tenants.map(tn => <option key={tn} value={tn}>{tn}</option>)}
        </select>
      </div>

      {isLoading ? <Skeleton className="h-32 w-full" /> : isError ? <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.common.uiRetry}</button>} /> : !hasTrafficData ? <EmptyState title={t.common.uiNoData} /> : <>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div className="glass-card p-4">
          <div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.traffic.totalRequests}</div>
          <div className="mt-1 font-operational text-2xl font-bold tracking-tight text-surface-900">{totalRequests.toLocaleString()}</div>
        </div>
        <div className="glass-card p-4">
          <div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.traffic.avgLatency}</div>
          <div className="mt-1 font-operational text-2xl font-bold tracking-tight text-surface-900">{avgLatency}</div>
        </div>
      </div>

      <div className="glass-card p-5">
        <h3 className="text-sm font-semibold text-surface-700 mb-4">{t.traffic.requestVolume}</h3>
        <div className="h-72">
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart data={chartData}>
              <defs><linearGradient id="colorReq" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#0a0a0a" stopOpacity={0.12} /><stop offset="100%" stopColor="#0a0a0a" stopOpacity={0} /></linearGradient></defs>
              <CartesianGrid strokeDasharray="3 3" stroke="#e5e0d9" />
              <XAxis dataKey="label" tick={{ fill: '#8e877d', fontSize: 11 }} axisLine={false} tickLine={false} />
              <YAxis tick={{ fill: '#8e877d', fontSize: 10 }} axisLine={false} tickLine={false} />
              <Tooltip contentStyle={{ background: 'white', border: '1px solid #e5e0d9', borderRadius: 8, color: '#403d38', boxShadow: '0 4px 16px rgba(41,39,36,0.08)' }} />
              <Area type="monotone" dataKey="requests" stroke="#0a0a0a" fill="url(#colorReq)" strokeWidth={2} />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      </div>

      <div className="glass-card p-5">
        <h3 className="text-sm font-semibold text-surface-700 mb-4">{t.traffic.latencyTrend}</h3>
        <div className="h-72">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={chartData}>
              <CartesianGrid strokeDasharray="3 3" stroke="#e5e0d9" />
              <XAxis dataKey="label" tick={{ fill: '#8e877d', fontSize: 11 }} axisLine={false} tickLine={false} />
              <YAxis tick={{ fill: '#8e877d', fontSize: 10 }} axisLine={false} tickLine={false} />
              <Tooltip contentStyle={{ background: 'white', border: '1px solid #e5e0d9', borderRadius: 8, color: '#403d38', boxShadow: '0 4px 16px rgba(41,39,36,0.08)' }} />
              <Bar dataKey="latencyMs" fill="#525252" radius={[4, 4, 0, 0]} />
            </BarChart>
          </ResponsiveContainer>
        </div>
        <div className="flex gap-4 mt-3 px-2 text-xs text-surface-400">
          <div className="flex items-center gap-1.5"><div className="h-3 w-3 rounded-sm bg-surface-900" /> {t.traffic.requests}</div>
          <div className="flex items-center gap-1.5"><div className="h-3 w-3 rounded-sm bg-surface-500" /> {t.traffic.latency}</div>
        </div>
      </div>
      </>}
    </div>
  );
}
