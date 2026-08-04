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
          <h1 className="text-xl font-semibold text-surface-900 sm:text-2xl">{t.live.title}</h1>
          <p className="text-sm text-surface-500 mt-1">{t.live.subtitle}</p>
        </div>
        <div className="flex items-center gap-3">
          {connected ? (<span className="badge badge-success flex items-center gap-1.5"><Wifi size={12} /> {t.live.live}</span>)
            : (<span className="badge badge-error flex items-center gap-1.5"><WifiOff size={12} /> {t.live.disconnected}</span>)}
          <button onClick={clear} className="btn-primary text-xs flex items-center gap-1.5"><Trash2 size={12} /> {t.live.clear}</button>
        </div>
      </div>

      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <div className="glass-card rounded-lg p-4"><div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.live.totalEvents}</div><div className="text-2xl font-bold text-surface-800 mt-1">{events.length}</div></div>
        <div className="glass-card p-4"><div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.live.ok200}</div><div className="text-2xl font-bold text-emerald-600 mt-1">{totalOk}</div></div>
        <div className="glass-card p-4"><div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.live.errors}</div><div className="text-2xl font-bold text-red-600 mt-1">{totalErr}</div></div>
        <div className="glass-card p-4"><div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.live.qps}</div><div className="text-2xl font-bold text-info mt-1">{qps}</div></div>
      </div>

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <div className="glass-card rounded-lg p-4"><div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.live.p50}</div><div className="text-2xl font-bold text-primary-600 mt-1">{p50 ? `${p50}ms` : '—'}</div></div>
        <div className="glass-card p-4"><div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.live.p99}</div><div className="text-2xl font-bold text-accent-600 mt-1">{p99 ? `${p99}ms` : '—'}</div></div>
        <div className="glass-card p-4"><div className="text-[11px] text-surface-400 uppercase tracking-wider">{t.live.successRate}</div><div className="text-2xl font-bold text-emerald-600 mt-1">{events.length > 0 ? `${Math.round((totalOk / events.length) * 100)}%` : '—'}</div></div>
      </div>

      <div className="space-y-2 max-h-[calc(100vh-420px)] overflow-y-auto pr-1">
        <AnimatePresence>
          {events.slice(0, 50).map((evt, i) => (
            <motion.div key={`${evt.request_id}-${i}`} initial={{ opacity: 0, x: -20, height: 0 }} animate={{ opacity: 1, x: 0, height: 'auto' }} exit={{ opacity: 0, x: 20 }} transition={{ duration: 0.25 }}
              className={`glass-card rounded-lg p-3.5 flex items-center gap-4 ${evt.status_code < 300 ? 'border-l-[3px] border-l-emerald-500' : 'border-l-[3px] border-l-red-500'}`}>
              <div className={`w-9 h-9 rounded-xl flex items-center justify-center text-xs font-bold ${evt.status_code < 300 ? 'bg-emerald-50 text-emerald-600 border border-emerald-200' : 'bg-red-50 text-red-600 border border-red-200'}`}>
                {evt.status_code}
              </div>
              <div className="flex-1 min-w-0"><div className="text-sm font-semibold text-surface-700">{evt.model}</div><div className="text-xs text-surface-400 font-mono">{evt.pool_id} <span className="text-surface-300">|</span> {evt.request_id.slice(0, 8)}</div></div>
              <div className="text-right flex-shrink-0">
                <div className={`text-sm font-bold tabular-nums ${evt.latency_ms > 2000 ? 'text-red-600' : evt.latency_ms > 500 ? 'text-amber-600' : 'text-emerald-600'}`}>{evt.latency_ms}ms</div>
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
