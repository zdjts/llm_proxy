import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Coins, DollarSign, Gauge, Zap } from 'lucide-react';
import { fetchUsage } from '@/lib/api';
import type { UsageSummary, UsageModelRow } from '@/lib/api';
import { useLocale } from '@/i18n/context';
import { EmptyState, ErrorState, Skeleton, StatCard } from '@/components/ui';
import { AreaChart, RankBars, StackBar } from '@/components/charts';

function fmtUsd(n: number): string {
  if (n > 0 && n < 0.01) return `$${n.toFixed(4)}`;
  return `$${n.toFixed(2)}`;
}

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toLocaleString();
}

function summaryCards(today: UsageSummary | undefined, t: ReturnType<typeof useLocale>['t']) {
  return [
    { label: t.usage.todayCost, value: today ? fmtUsd(today.cost_usd) : '—', icon: DollarSign },
    { label: t.usage.todayRequests, value: today ? today.requests.toLocaleString() : '—', icon: Zap },
    { label: t.usage.todayAvgLatency, value: today && today.requests > 0 ? `${today.avg_latency_ms}ms` : '—', icon: Gauge },
    { label: t.usage.todayErrors, value: today ? today.errors.toLocaleString() : '—', icon: Coins },
  ];
}

function ModelTable({ rows }: { rows: UsageModelRow[] }) {
  const { t } = useLocale();
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr>
            {[t.usage.thModel, t.usage.thRequests, t.usage.thErrors, t.usage.thTokens, t.usage.thCacheHit, t.usage.thAvgLatency, t.usage.thCost].map((header, i) => (
              <th key={header} className={`border-b border-surface-200 p-3 font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400 ${i === 0 ? 'text-left' : 'text-right'}`}>{header}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.model} className="border-b border-surface-100 last:border-0 hover:bg-surface-50 transition-colors">
              <td className="p-3 font-medium text-surface-700">{row.model}</td>
              <td className="p-3 text-right tabular-nums text-surface-600">{row.requests.toLocaleString()}</td>
              <td className={`p-3 text-right tabular-nums ${row.errors > 0 ? 'font-medium text-danger' : 'text-surface-400'}`}>{row.errors}</td>
              <td className="p-3 text-right tabular-nums text-surface-600">{fmtTokens(row.prompt_tokens)} / {fmtTokens(row.completion_tokens)} / {fmtTokens(row.cached_tokens)}</td>
              <td className="p-3 text-right tabular-nums">
                <span className={`badge ${row.cache_hit_rate >= 30 ? 'badge-success' : row.cache_hit_rate > 0 ? 'badge-info' : 'badge-info'}`}>
                  {row.cache_hit_rate.toFixed(1)}%
                </span>
              </td>
              <td className="p-3 text-right tabular-nums text-surface-600">{row.avg_latency_ms}ms</td>
              <td className="p-3 text-right font-semibold tabular-nums text-surface-900">{fmtUsd(row.cost_usd)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function UsagePage() {
  const { t } = useLocale();
  const [hours, setHours] = useState(168);
  const [tenant, setTenant] = useState('');
  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['usage', hours, tenant],
    queryFn: () => fetchUsage(hours, tenant || undefined),
    refetchInterval: 60000,
  });

  const trend = data?.trend ?? [];
  const totalTokens = (data?.total.prompt_tokens ?? 0) + (data?.total.completion_tokens ?? 0);

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div>
          <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.usage.title}</h1>
          <p className="mt-1 text-[14.5px] text-surface-600">{t.usage.subtitle}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex gap-1 rounded-lg bg-surface-100 p-0.5">
            {[[24, t.usage.range24h], [168, t.usage.range7d], [720, t.usage.range30d]].map(([h, label]) => (
              <button key={h} type="button" onClick={() => setHours(h as number)}
                className={`rounded-md px-3 py-1.5 text-xs font-medium transition-all duration-200 ${
                  hours === h ? 'border border-surface-200 bg-white text-surface-900 shadow-sm' : 'text-surface-500 hover:text-surface-900'
                }`}>{label}</button>
            ))}
          </div>
          <select value={tenant} onChange={(e) => setTenant(e.target.value)} className="input-glass">
            <option value="">{t.usage.allTenants}</option>
            {data?.tenants.map((tn) => <option key={tn} value={tn}>{tn}</option>)}
          </select>
        </div>
      </div>

      {isLoading ? <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4"><Skeleton className="h-24" /><Skeleton className="h-24" /><Skeleton className="h-24" /><Skeleton className="h-24" /></div>
        : isError ? <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.common.uiRetry}</button>} />
          : !data ? null : <>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {summaryCards(data.today, t).map((card) => (
              <StatCard key={card.label} label={card.label} value={card.value} icon={card.icon} />
            ))}
          </div>

          <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1.4fr_0.6fr]">
            <div className="glass-card p-5">
              <h3 className="mb-4 text-sm font-semibold text-surface-700">{t.usage.costTrend}</h3>
              <AreaChart
                data={trend}
                series={[{ key: 'cost_usd', label: t.usage.costLabel, color: '#b8413a' }]}
                height={260}
              />
            </div>

            <div className="glass-card p-5">
              <div className="font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{t.usage.totalTokens}</div>
              <div className="mt-1 font-operational text-3xl font-bold tracking-tight text-surface-900">{fmtTokens(totalTokens)}</div>
              <div className="mt-5">
                <StackBar
                  segments={[
                    { label: t.usage.promptLabel, value: data.total.prompt_tokens - data.total.cached_tokens, color: '#0a0a0a' },
                    { label: t.usage.cacheLabel, value: data.total.cached_tokens, color: '#3d7a4e' },
                    { label: t.usage.completionLabel, value: data.total.completion_tokens, color: '#a3a3a3' },
                  ]}
                />
              </div>
              <div className="mt-6 space-y-2 border-t border-surface-100 pt-4 text-sm">
                <div className="flex justify-between"><span className="text-surface-500">{t.usage.promptTokens}</span><span className="font-operational tabular-nums text-surface-800">{data.total.prompt_tokens.toLocaleString()}</span></div>
                <div className="flex justify-between"><span className="text-surface-500">{t.usage.completionTokens}</span><span className="font-operational tabular-nums text-surface-800">{data.total.completion_tokens.toLocaleString()}</span></div>
                <div className="flex justify-between"><span className="text-surface-500">{t.usage.cachedTokens}</span><span className="font-operational tabular-nums text-success-dark">{data.total.cached_tokens.toLocaleString()}</span></div>
              </div>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
            <div className="glass-card p-5">
              <h3 className="mb-4 text-sm font-semibold text-surface-700">{t.usage.requestTrend}</h3>
              <AreaChart
                data={trend}
                series={[{ key: 'requests', label: t.usage.requestsLabel, color: '#0a0a0a' }]}
                barSeries={{ key: 'errors', label: t.usage.thErrors, color: '#b04436' }}
                height={240}
              />
            </div>

            <div className="glass-card p-5">
              <h3 className="mb-4 text-sm font-semibold text-surface-700">{t.usage.latencyTrend}</h3>
              <AreaChart
                data={trend}
                series={[{ key: 'avg_latency_ms', label: t.usage.latencyLabel, color: '#4f7887' }]}
                height={240}
              />
            </div>
          </div>

          <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
            <div className="glass-card p-5">
              <h3 className="mb-4 text-sm font-semibold text-surface-700">{t.usage.tokenTrend}</h3>
              <AreaChart
                data={trend}
                series={[
                  { key: 'prompt_tokens', label: t.usage.promptLabel, color: '#0a0a0a' },
                  { key: 'cached_tokens', label: t.usage.cacheLabel, color: '#3d7a4e' },
                  { key: 'completion_tokens', label: t.usage.completionLabel, color: '#a3a3a3' },
                ]}
                height={240}
              />
            </div>

            <div className="glass-card p-5">
              <h3 className="mb-4 text-sm font-semibold text-surface-700">{t.usage.modelBreakdown}</h3>
              {data.models.length > 0
                ? <RankBars rows={data.models.slice(0, 8).map((m) => ({ label: m.model, value: m.requests }))} />
                : <EmptyState title={t.common.uiNoData} className="border-0 py-8" />}
              {data.models.length > 0 && (
                <div className="mt-5">
                  <StackBar
                    segments={data.models.slice(0, 8).map((m, i) => ({
                      label: m.model,
                      value: m.requests,
                      color: ['#0a0a0a', '#525252', '#767673', '#a3a3a3', '#c4c4bf', '#b8413a', '#4f7887', '#3d7a4e'][i % 8],
                    }))}
                  />
                </div>
              )}
            </div>
          </div>

          <div className="glass-card overflow-hidden p-1">
            {data.models.length === 0 ? <EmptyState title={t.common.uiNoData} className="m-2 border-0" /> : <ModelTable rows={data.models} />}
          </div>
        </>}
    </div>
  );
}
