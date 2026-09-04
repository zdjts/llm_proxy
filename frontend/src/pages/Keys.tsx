import { useQuery } from '@tanstack/react-query';
import { fetchKeys } from '@/lib/api';
import { ShieldCheck, ShieldX, Activity } from 'lucide-react';
import { motion } from 'framer-motion';
import { useLocale } from '@/i18n/context';
import { EmptyState, ErrorState, Skeleton } from '@/components/ui';

export function KeysPage() {
  const { t } = useLocale();
  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['keys'],
    queryFn: fetchKeys,
    refetchInterval: 15000,
  });

  const totalKeys = data?.pools.reduce((sum, p) => sum + p.keys.length, 0) ?? 0;
  const totalHealthy = data?.pools.reduce((sum, p) => sum + p.keys.filter(k => k.healthy).length, 0) ?? 0;
  const healthPct = totalKeys > 0 ? Math.round((totalHealthy / totalKeys) * 100) : 0;

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div>
          <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.keys.title}</h1>
          <p className="mt-1 text-[14.5px] text-surface-600">{t.keys.subtitle}</p>
        </div>
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 rounded-md border border-surface-200 bg-white px-3 py-2">
            <Activity size={14} className="text-surface-800" />
            <span className="text-sm font-semibold text-surface-700">{t.keys.healthyPct.replace('{pct}', String(healthPct))}</span>
            <span className="text-xs text-surface-400">{t.keys.healthyLabel}</span>
          </div>
        </div>
      </div>

      {isLoading && <Skeleton className="h-32 w-full" />}
      {isError && <ErrorState action={<button type="button" className="btn-secondary text-xs" onClick={() => refetch()}>{t.common.uiRetry}</button>} />}
      {!isLoading && !isError && data?.pools.map(pool => {
        const healthy = pool.keys.filter(k => k.healthy).length;
        const total = pool.keys.length;
        const pct = total > 0 ? Math.round((healthy / total) * 100) : 0;

        return (
          <div key={pool.pool_id} className="glass-card p-5">
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-3">
                <div className={`h-2 w-2 rounded-full ${pct > 70 ? 'bg-surface-900' : pct > 30 ? 'bg-surface-500' : 'bg-surface-300'}`} />
                <h3 className="text-lg font-semibold text-surface-800">{pool.pool_id}</h3>
              </div>
              <div className="flex items-center gap-2">
                <span className={`badge ${pct > 70 ? 'badge-success' : pct > 30 ? 'badge-warn' : 'badge-error'}`}>
                  {t.keys.healthy.replace('{healthy}', String(healthy)).replace('{total}', String(total))}
                </span>
              </div>
            </div>

            <div className="grid gap-2" role="list">
              {pool.keys.map((key, idx) => (
                <motion.div
                  key={key.key_hash}
                  initial={{ opacity: 0, x: -10 }}
                  animate={{ opacity: 1, x: 0 }}
                  transition={{ delay: idx * 0.03 }}
                  className="flex items-center gap-4 border-t border-surface-100 p-3.5 transition-colors first:border-t-0 hover:bg-surface-50"
                >
                  {key.healthy ? (
                    <ShieldCheck size={18} className="shrink-0 text-surface-800" />
                  ) : (
                    <ShieldX size={18} className="shrink-0 text-surface-500" />
                  )}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-xs text-surface-500">{key.key_hash}</span>
                      <span className={`badge ${key.healthy ? 'badge-success' : 'badge-error'}`}>
                        {key.healthy ? t.keys.healthyBadge : t.keys.excluded}
                      </span>
                    </div>
                    {key.sparkline.length > 0 && (
                      <div className="flex items-end gap-[1px] h-6 mt-1.5">
                        {key.sparkline.map((h, i) => (
                          <div key={i} className="flex-1 rounded-[1px] bg-surface-400 transition-colors hover:bg-surface-700"
                            style={{ height: `${Math.max(1, h)}px` }} />
                        ))}
                      </div>
                    )}
                  </div>
                  <div className="text-right flex-shrink-0 space-y-0.5">
                    <div className="text-xs text-surface-400">{t.keys.weight.replace('{weight}', String(key.weight))}</div>
                    <div className="font-operational text-xs font-medium text-surface-700">{key.success_rate}</div>
                  </div>
                </motion.div>
              ))}
            </div>
          </div>
        );
      })}

      {!isLoading && !isError && (!data?.pools || data.pools.length === 0) && (
        <EmptyState title={t.keys.noPools} />
      )}
    </div>
  );
}
