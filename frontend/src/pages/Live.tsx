import { useLiveSocket } from '@/hooks/useWebSocket';
import { useState, useEffect, useMemo } from 'react';
import { Wifi, WifiOff, Trash2, BarChart3 } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { useLocale } from '@/i18n/context';

export function LivePage() {
  const { t } = useLocale();
  const { connected, events, clear } = useLiveSocket();
  const [qps, setQps] = useState(0);
  const [qpsCount, setQpsCount] = useState(0);

  useEffect(() => { const timer = setInterval(() => { setQps(qpsCount); setQpsCount(0); }, 1000); return () => clearInterval(timer); }, [qpsCount]);
  useEffect(() => { if (events.length > 0) setQpsCount(c => c + 1); }, [events]);

  const totalOk = useMemo(() => events.filter(e => e.status_code >= 200 && e.status_code < 300).length, [events]);
  const totalErr = useMemo(() => events.filter(e => e.status_code >= 300).length, [events]);
  const latencies = useMemo(() => events.map(e => e.latency_ms).filter(l => l > 0).slice(0, 100), [events]);
  const p50 = useMemo(() => latencies.length > 0 ? [...latencies].sort((a, b) => a - b)[Math.floor(latencies.length * 0.5)] : null, [latencies]);
  const p99 = useMemo(() => latencies.length > 0 ? [...latencies].sort((a, b) => a - b)[Math.floor(latencies.length * 0.99)] : null, [latencies]);

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div>
          <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.live.title}</h1>
          <p className="mt-1 text-[14.5px] text-surface-600">{t.live.subtitle}</p>
        </div>
        <div className="flex items-center gap-3">
          {connected ? (<span className="badge badge-success flex items-center gap-1.5"><Wifi size={12} /> {t.live.live}</span>)
            : (<span className="badge badge-error flex items-center gap-1.5"><WifiOff size={12} /> {t.live.disconnected}</span>)}
          <button onClick={clear} className="btn-primary text-xs flex items-center gap-1.5"><Trash2 size={12} /> {t.live.clear}</button>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {[[t.live.totalEvents, events.length], [t.live.ok200, totalOk], [t.live.errors, totalErr], [t.live.qps, qps]].map(([label, value]) => <div key={String(label)} className="glass-card p-4"><div className="text-xs font-medium text-surface-500">{label}</div><div className="mt-1 font-operational text-2xl font-bold tracking-tight text-surface-900">{value}</div></div>)}
      </div>

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        {[[t.live.p50, p50 ? `${p50}ms` : '—'], [t.live.p99, p99 ? `${p99}ms` : '—'], [t.live.successRate, events.length > 0 ? `${Math.round((totalOk / events.length) * 100)}%` : '—']].map(([label, value]) => <div key={String(label)} className="glass-card p-4"><div className="text-xs font-medium text-surface-500">{label}</div><div className="mt-1 font-operational text-2xl font-bold tracking-tight text-surface-900">{value}</div></div>)}
      </div>

      <div className="space-y-2 max-h-[calc(100vh-420px)] overflow-y-auto pr-1">
        <AnimatePresence>
          {events.slice(0, 50).map((evt, i) => (
            <motion.div key={`${evt.request_id}-${i}`} initial={{ opacity: 0, x: -20, height: 0 }} animate={{ opacity: 1, x: 0, height: 'auto' }} exit={{ opacity: 0, x: 20 }} transition={{ duration: 0.25 }}
              className="glass-card flex items-center gap-4 p-3.5">
              <div className="flex h-9 w-9 items-center justify-center rounded-xl border border-surface-200 bg-surface-50 text-xs font-bold text-surface-900">
                {evt.status_code}
              </div>
              <div className="flex-1 min-w-0"><div className="text-sm font-semibold text-surface-700">{evt.model}</div><div className="text-xs text-surface-400 font-mono">{evt.pool_id} <span className="text-surface-300">|</span> {evt.request_id.slice(0, 8)}</div></div>
              <div className="text-right flex-shrink-0">
                <div className="font-operational text-sm font-bold tabular-nums text-surface-900">{evt.latency_ms}ms</div>
                {evt.tokens != null && <div className="text-xs text-surface-400">{t.live.tokens.replace('{n}', String(evt.tokens))}</div>}
              </div>
            </motion.div>
          ))}
        </AnimatePresence>
        {events.length === 0 && (
          <div className="glass-card p-12 text-center text-surface-400"><BarChart3 size={32} className="mx-auto mb-3 text-surface-300" /><p className="text-sm">{t.live.waiting}</p></div>
        )}
      </div>
    </div>
  );
}
