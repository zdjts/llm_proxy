import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { EmptyState, ErrorState, Skeleton } from '@/components/ui';
import { useLocale } from '@/i18n/context';

export function BudgetPage() {
  const { t } = useLocale();
  const { isLoading, isError, refetch } = useQuery({ queryKey: ['orgs'], queryFn: () => api.get('/admin/api/orgs').then(r => r.data) });
  return (
    <div className="space-y-6">
      <div><h1 className="text-xl font-semibold text-surface-900 sm:text-2xl">{t.configAdmin.budgetTitle}</h1><p className="mt-1 text-sm text-surface-500">{t.configAdmin.budgetSubtitle}</p></div>
      {isLoading ? <Skeleton className="h-64" /> : isError ? <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.adminUi.retry}</button>} /> : <EmptyState title={t.adminUi.unavailable} description={t.configAdmin.budgetHint} />}
    </div>
  );
}
