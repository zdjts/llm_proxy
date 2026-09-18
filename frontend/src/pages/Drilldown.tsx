import { useQuery } from '@tanstack/react-query';
import { useSearchParams, Link } from 'react-router-dom';
import { fetchDrilldown } from '@/lib/api';
import { ArrowLeft } from 'lucide-react';
import { useLocale } from '@/i18n/context';
import { EmptyState, ErrorState, Skeleton } from '@/components/ui';
import { AreaChart } from '@/components/charts';

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
      label: l.text,
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
            <AreaChart
              data={chartData}
              series={[
                { key: 'requests', label: t.drilldown.requests, color: '#0a0a0a' },
                { key: 'promptTokens', label: t.drilldown.promptTokens, color: '#525252' },
                { key: 'completionTokens', label: t.drilldown.completionTokens, color: '#a3a3a3' },
              ]}
              height={320}
            />
          </div>
        </div>
      )}
      {!isLoading && !isError && chartData.length === 0 && <EmptyState title={t.common.uiNoData} />}
    </div>
  );
}
