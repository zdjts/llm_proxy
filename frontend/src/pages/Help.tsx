import { useQuery } from '@tanstack/react-query';
import axios from 'axios';
import { useLocale } from '@/i18n/context';
import { EmptyState, ErrorState, Skeleton } from '@/components/ui';

export function HelpPage() {
  const { t } = useLocale();
  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['help'],
    queryFn: async () => { const { data } = await axios.get<{ runbook: string }>('/admin/help?format=json'); return data; },
  });

  return (
    <div className="space-y-6">
      <div className="border-b border-surface-200 pb-5">
        <h1 className="text-xl font-semibold text-surface-900 sm:text-2xl">{t.help.title}</h1>
        <p className="text-sm text-surface-500 mt-1">{t.help.subtitle}</p>
      </div>
      {isLoading ? <Skeleton className="h-32 w-full" /> : isError ? <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.common.uiRetry}</button>} /> : data?.runbook?.trim() ? <div className="glass-card rounded-lg overflow-hidden p-5 sm:p-6"><div className="prose max-w-none"><pre className="text-sm text-surface-600 whitespace-pre-wrap font-mono leading-relaxed bg-transparent p-0">{data.runbook}</pre></div></div> : <EmptyState title={t.help.empty} />}
    </div>
  );
}
