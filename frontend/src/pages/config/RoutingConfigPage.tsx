import { useQuery } from '@tanstack/react-query';
import { fetchRouting } from '@/lib/api';
import { Card, EmptyState, ErrorState, Skeleton, Badge, Button } from '@/components/ui';
import { useLocale } from '@/i18n/context';

export function RoutingConfigPage() {
  const { t } = useLocale();
  const { data, isLoading, isError, refetch } = useQuery({ queryKey: ['routing'], queryFn: fetchRouting });
  const entries = data?.routing || [];
  return (
    <div className="space-y-6">
      <div className="border-b border-surface-200 pb-5"><h1 className="text-xl font-semibold text-surface-900 sm:text-2xl">{t.configAdmin.routingTitle}</h1><p className="mt-1 text-sm text-surface-500">{t.configAdmin.routingSubtitle}</p></div>
      {isLoading ? <Skeleton className="h-64" /> : isError ? <ErrorState action={<Button size="sm" variant="secondary" onClick={() => refetch()}>{t.adminUi.retry}</Button>} /> : entries.length === 0 ? <EmptyState title={t.adminUi.noRouting} /> : <div className="grid gap-3">
        {entries.map((e: { logical_model: string; pool_id: string }) => (
          <Card key={e.logical_model} className="rounded-lg flex items-center justify-between gap-3 px-5 py-4">
            <span className="min-w-0 truncate font-mono text-sm font-semibold">{e.logical_model}</span>
            <span className="text-surface-400 text-sm" aria-hidden="true">→</span>
            <Badge variant="info">{e.pool_id}</Badge>
          </Card>
        ))}
      </div>}
    </div>
  );
}
