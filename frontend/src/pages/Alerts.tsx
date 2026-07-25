import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { fetchAlerts } from '@/lib/api';
import { AlertTriangle, Clock, Zap, Ban, Download } from 'lucide-react';
import { csvDownload } from '@/lib/utils';
import { useLocale } from '@/i18n/context';

const typeIcons: Record<string, typeof AlertTriangle> = { UpstreamError: AlertTriangle, LatencySpike: Clock, RateLimited: Zap, PoolExhausted: Ban };
const typeColors: Record<string, string> = { UpstreamError: 'badge-error', LatencySpike: 'badge-warn', RateLimited: 'badge-purple', PoolExhausted: 'badge-info' };

export function AlertsPage() {
  const { t } = useLocale();
  const [type, setType] = useState('');
  const [tenant, setTenant] = useState('');
  const { data } = useQuery({
    queryKey: ['alerts', type, tenant],
    queryFn: () => fetchAlerts(type || undefined, tenant || undefined),
    refetchInterval: 15000,
  });

  const handleCsvExport = () => {
    if (!data?.events) return;
    const rows = data.events.map(e => [String(e.id), new Date(e.ts * 1000).toISOString(), e.event_type, e.pool_id || '', e.tenant_id || '', e.model || '', e.error_code || '', e.msg]);
    csvDownload('alerts.csv', rows, [t.alerts.thId, t.alerts.thTime, t.alerts.thType, t.alerts.thPool, t.alerts.thTenant, t.alerts.thModel, t.alerts.thErrorCode, t.alerts.thMessage]);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gradient">{t.alerts.title}</h1>
          <p className="text-sm text-surface-500 mt-1">{t.alerts.subtitle.replace('{count}', String(data?.event_count ?? 0))}</p>
        </div>
        <button onClick={handleCsvExport} className="btn-gold flex items-center gap-1.5 text-xs" disabled={!data?.events?.length}>
          <Download size={13} /> {t.alerts.csv}
        </button>
      </div>

      <div className="glass-card p-4 flex flex-wrap gap-3">
        <select value={type} onChange={e => setType(e.target.value)} className="input-glass">
          <option value="">{t.alerts.allTypes}</option>
          {['UpstreamError', 'LatencySpike', 'RateLimited', 'PoolExhausted'].map(tp => <option key={tp} value={tp}>{tp}</option>)}
        </select>
        <input placeholder={t.common.placeholder_tenant} value={tenant} onChange={e => setTenant(e.target.value)} className="input-glass w-40" />
      </div>

      <div className="glass-card overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-surface-100 bg-surface-50/50">
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.alerts.thId}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.alerts.thTime}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.alerts.thType}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.alerts.thPool}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.alerts.thTenant}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.alerts.thModel}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.alerts.thErrorCode}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.alerts.thMessage}</th>
              </tr>
            </thead>
            <tbody>
              {data?.events.map(evt => {
                const Icon = typeIcons[evt.event_type] || AlertTriangle;
                return (
                  <tr key={evt.id} className="border-b border-surface-50 hover:bg-surface-50/50 transition-colors">
                    <td className="p-3 text-surface-400 text-xs font-mono">{evt.id}</td>
                    <td className="p-3 text-surface-500 text-xs whitespace-nowrap">{new Date(evt.ts * 1000).toLocaleString()}</td>
                    <td className="p-3"><span className={`badge ${typeColors[evt.event_type] || 'badge-info'} flex items-center gap-1 w-fit`}><Icon size={12} /> {evt.event_type}</span></td>
                    <td className="p-3 text-surface-500">{evt.pool_id || '—'}</td>
                    <td className="p-3 text-surface-500">{evt.tenant_id || '—'}</td>
                    <td className="p-3 text-surface-500">{evt.model || '—'}</td>
                    <td className="p-3 text-red-600">{evt.error_code || '—'}</td>
                    <td className="p-3 text-surface-600 max-w-[300px] truncate">{evt.msg}</td>
                  </tr>
                );
              })}
              {(!data?.events || data.events.length === 0) && (
                <tr><td colSpan={8} className="p-12 text-center text-surface-400"><AlertTriangle size={24} className="mx-auto mb-2 text-surface-300" />{t.alerts.noData}</td></tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
