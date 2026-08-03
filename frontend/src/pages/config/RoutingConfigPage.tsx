import { useQuery } from '@tanstack/react-query';
import { fetchRouting } from '@/lib/api';
import { Card, Skeleton, Badge } from '@/components/ui';

export function RoutingConfigPage() {
  const { data, isLoading } = useQuery({ queryKey: ['routing'], queryFn: fetchRouting });
  if (isLoading) return <Skeleton className="h-64" />;
  const entries = data?.routing || [];
  return (
    <div className="space-y-6">
      <div><h1 className="text-2xl font-bold text-surface-800">Model Routing</h1><p className="text-sm text-surface-400 mt-1">Map logical model names to upstream pools</p></div>
      <div className="grid gap-3">
        {entries.map((e: { logical_model: string; pool_id: string }) => (
          <Card key={e.logical_model} className="flex items-center justify-between py-4 px-5">
            <span className="font-mono text-sm font-semibold">{e.logical_model}</span>
            <span className="text-surface-400 text-sm">→</span>
            <Badge variant="info">{e.pool_id}</Badge>
          </Card>
        ))}
      </div>
    </div>
  );
}
