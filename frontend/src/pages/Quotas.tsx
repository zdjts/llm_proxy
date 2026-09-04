import { useQuery } from '@tanstack/react-query';
import { fetchQuotas } from '@/lib/api';
import { Gauge, Infinity } from 'lucide-react';
import { motion } from 'framer-motion';
import { useLocale } from '@/i18n/context';
import { EmptyState, ErrorState, Skeleton } from '@/components/ui';

export function QuotasPage() {
  const { t } = useLocale();
  const { data, isLoading, isError, refetch } = useQuery({ queryKey: ['quotas'], queryFn: fetchQuotas, refetchInterval: 30000 });

  return (
    <div className="space-y-6">
      <div className="border-b border-surface-200 pb-5">
        <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.quotas.title}</h1>
        <p className="mt-1 text-[14.5px] text-surface-600">{t.quotas.subtitle}</p>
      </div>

      {isLoading ? <Skeleton className="h-32 w-full" /> : isError ? <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.common.uiRetry}</button>} /> : data && data.length > 0 ? (
        <div className="grid gap-4">
          {data.map(q => {
            const tokenPct = q.daily_tokens_limit ? Math.min(100, (q.daily_tokens_used / q.daily_tokens_limit) * 100) : 0;
            const reqPct = q.monthly_requests_limit ? Math.min(100, (q.monthly_requests_used / q.monthly_requests_limit) * 100) : 0;

            return (
              <motion.div key={q.tenant_id} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} className="glass-card p-5">
                <div className="flex items-center gap-2.5 mb-5">
                  <div className="flex h-9 w-9 items-center justify-center rounded-xl border border-surface-200 bg-surface-50"><Gauge size={17} className="text-surface-800" /></div>
                  <h3 className="text-lg font-semibold text-surface-800">{q.tenant_id}</h3>
                </div>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                  <div>
                    <div className="flex justify-between text-xs mb-1.5">
                      <span className="text-surface-500 font-medium">{t.quotas.dailyTokens}</span>
                      <span className="text-surface-600 tabular-nums">
                        <span className="font-semibold">{q.daily_tokens_used.toLocaleString()}</span>
                        <span className="text-surface-400"> / {q.daily_tokens_limit?.toLocaleString() ?? <Infinity size={12} className="inline" />}</span>
                      </span>
                    </div>
                    <div className="h-2 overflow-hidden rounded-full bg-surface-100">
                      <motion.div initial={{ width: 0 }} animate={{ width: `${q.daily_tokens_limit ? tokenPct : Math.min(100, (q.daily_tokens_used / 10000) * 100)}%` }} transition={{ duration: 0.6, ease: 'easeOut' }}
                        className={`h-full rounded-full ${q.daily_tokens_limit ? (tokenPct > 95 ? 'bg-surface-900' : tokenPct > 80 ? 'bg-surface-700' : 'bg-surface-500') : 'bg-surface-300'}`} />
                    </div>
                    {tokenPct >= 80 && q.daily_tokens_limit && (
                      <p className={`text-[11px] mt-1.5 text-surface-600 font-medium`}>{tokenPct > 95 ? t.quotas.critical : t.quotas.warning}</p>
                    )}
                  </div>
                  <div>
                    <div className="flex justify-between text-xs mb-1.5">
                      <span className="text-surface-500 font-medium">{t.quotas.monthlyRequests}</span>
                      <span className="text-surface-600 tabular-nums">
                        <span className="font-semibold">{q.monthly_requests_used.toLocaleString()}</span>
                        <span className="text-surface-400"> / {q.monthly_requests_limit?.toLocaleString() ?? <Infinity size={12} className="inline" />}</span>
                      </span>
                    </div>
                    <div className="h-2 overflow-hidden rounded-full bg-surface-100">
                      <motion.div initial={{ width: 0 }} animate={{ width: `${q.monthly_requests_limit ? reqPct : Math.min(100, (q.monthly_requests_used / 1000) * 100)}%` }} transition={{ duration: 0.6, ease: 'easeOut' }}
                        className={`h-full rounded-full ${q.monthly_requests_limit ? (reqPct > 95 ? 'bg-surface-900' : reqPct > 80 ? 'bg-surface-700' : 'bg-surface-500') : 'bg-surface-300'}`} />
                    </div>
                    {reqPct >= 80 && q.monthly_requests_limit && (
                      <p className={`text-[11px] mt-1.5 text-surface-600 font-medium`}>{reqPct > 95 ? t.quotas.critical : t.quotas.warning}</p>
                    )}
                  </div>
                </div>
              </motion.div>
            );
          })}
        </div>
      ) : (
        <EmptyState title={t.quotas.noData} description={t.quotas.noDataHint} />
      )}
    </div>
  );
}
