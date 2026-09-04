import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { EmptyState, ErrorState, Skeleton } from '@/components/ui';
import { useLocale } from '@/i18n/context';

export function BudgetPage() {
  const { t } = useLocale();
  const { isLoading, isError, refetch } = useQuery({ queryKey: ['orgs'], queryFn: () => api.get('/admin/api/orgs').then(r => r.data) });
  return (
    <div className="space-y-6">
      <div className="border-b border-surface-200 pb-5"><h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.configAdmin.budgetTitle}</h1><p className="mt-1 text-[14.5px] text-surface-600">{t.configAdmin.budgetSubtitle}</p></div>
      {isLoading ? <Skeleton className="h-64" /> : isError ? <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.adminUi.retry}</button>} /> : <EmptyState title={t.adminUi.unavailable} description={t.configAdmin.budgetHint} />}
    </div>
  );
}
