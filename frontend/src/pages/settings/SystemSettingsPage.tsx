import { Card, ErrorState, Skeleton } from '@/components/ui';
import { useLocale } from '@/i18n/context';
import { useQuery } from '@tanstack/react-query';
import { fetchStatus } from '@/lib/api';

export function SystemSettingsPage() {
  const { t } = useLocale();
  const { data: status, isLoading, isError, refetch } = useQuery({ queryKey: ['status'], queryFn: fetchStatus });
  if (isLoading) return <div className="space-y-4"><Skeleton className="h-8 w-48" /><Skeleton className="h-48" /></div>;
  if (isError) return <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.adminUi.retry}</button>} />;
  return (
    <div className="space-y-6">
      <div className="border-b border-surface-200 pb-5"><h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.configAdmin.systemTitle}</h1><p className="mt-1 text-[14.5px] text-surface-600">{t.configAdmin.systemSubtitle}</p></div>
      <Card>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
          {[[t.configAdmin.uptime, status?.uptime_secs], [t.configAdmin.totalRequests, status?.requests_total], [t.configAdmin.failedRequests, status?.requests_failed], [t.configAdmin.cacheHits, status?.cache_hits], [t.configAdmin.activeConnections, status?.active_connections], [t.configAdmin.retries, status?.retries], [t.configAdmin.keyDemotions, status?.key_demotions], [t.configAdmin.upstream5xx, status?.upstream_5xx], [t.configAdmin.upstream4xx, status?.upstream_4xx], [t.configAdmin.alertCount, status?.alert_count]].map(([label, val]) => (
            <div key={label as string}>
              <div className="font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{label}</div>
              <div className="mt-1 font-operational text-xl font-bold text-surface-900">{(val as number)?.toLocaleString() ?? '—'}</div>
            </div>
          ))}
        </div>
      </Card>
    </div>
  );
}
