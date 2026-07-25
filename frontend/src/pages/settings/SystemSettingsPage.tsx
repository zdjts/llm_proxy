import { Card, Skeleton } from '@/components/ui';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';

export function SystemSettingsPage() {
  const { data: status, isLoading } = useQuery({ queryKey: ['status'], queryFn: () => api.get('/admin/api/status').then(r => r.data) });
  if (isLoading) return <div className="space-y-4"><Skeleton className="h-8 w-48" /><Skeleton className="h-48" /></div>;
  return (
    <div className="space-y-6">
      <div><h1 className="text-2xl font-bold text-surface-800">System</h1><p className="text-sm text-surface-400 mt-1">Gateway status and configuration</p></div>
      <Card>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
          {[['Uptime (s)', status?.uptime_secs], ['Total Requests', status?.requests_total], ['Failed', status?.requests_failed], ['Cache Hits', status?.cache_hits], ['Active Conns', status?.active_connections], ['Retries', status?.retries], ['Key Demotions', status?.key_demotions], ['Upstream 5xx', status?.upstream_5xx], ['Upstream 4xx', status?.upstream_4xx], ['Alert Count', status?.alert_count]].map(([label, val]) => (
            <div key={label as string}>
              <div className="text-surface-400 text-xs uppercase tracking-wider">{label}</div>
              <div className="text-lg font-semibold font-mono">{(val as number)?.toLocaleString() ?? '—'}</div>
            </div>
          ))}
        </div>
      </Card>
    </div>
  );
}
