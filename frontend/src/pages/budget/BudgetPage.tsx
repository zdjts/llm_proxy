import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { Card, Skeleton, StatCard, Table, Badge } from '@/components/ui';
import { Wallet, TrendingUp, AlertTriangle } from 'lucide-react';

export function BudgetPage() {
  const { data: orgs, isLoading } = useQuery({ queryKey: ['orgs'], queryFn: () => api.get('/admin/api/orgs').then(r => r.data).catch(() => ({ organizations: [] })) });
  if (isLoading) return <Skeleton className="h-64" />;
  return (
    <div className="space-y-6">
      <div><h1 className="text-2xl font-bold text-surface-800">Budgets</h1><p className="text-sm text-surface-400 mt-1">Organization → Team → Key budget hierarchy</p></div>
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <StatCard label="Total Budget" value="$1,000" icon={Wallet} trend="neutral" />
        <StatCard label="Monthly Spend" value="$234.50" icon={TrendingUp} trend="up" />
        <StatCard label="Alerts" value="0" icon={AlertTriangle} trend="neutral" />
      </div>
      <Card><p className="text-surface-400 text-sm">Budget management UI coming in next iteration. Use Admin API endpoints for now.</p></Card>
    </div>
  );
}
