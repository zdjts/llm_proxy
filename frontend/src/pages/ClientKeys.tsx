import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { fetchClientKeys, addClientKey, updateClientKey, deleteClientKey, rotateClientKey, apiError } from '@/lib/api';
import { Plus, Trash2, RefreshCw, Check, X, Key } from 'lucide-react';
import { useLocale } from '@/i18n/context';
import { Button, EmptyState, ErrorState, Skeleton, Table } from '@/components/ui';

export function ClientKeysPage() {
  const { t } = useLocale();
  const queryClient = useQueryClient();
  const { data, isLoading, isError, refetch } = useQuery({ queryKey: ['client-keys'], queryFn: fetchClientKeys, refetchInterval: 30000 });
  const [error, setError] = useState('');
  const [showAdd, setShowAdd] = useState(false);
  const [newKey, setNewKey] = useState({ key: '', tenant: 'default', label: '' });
  const [rotateKey, setRotateKey] = useState({ hash: '', newKey: '' });
  const clearNewKey = () => setNewKey({ key: '', tenant: 'default', label: '' });
  const closeAdd = () => { clearNewKey(); setShowAdd(false); };
  const closeRotate = () => setRotateKey({ hash: '', newKey: '' });

  const addMut = useMutation({
    mutationFn: () => addClientKey(newKey.key, newKey.tenant, newKey.label),
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['client-keys'] }); closeAdd(); },
    onError: () => { setNewKey((value) => ({ ...value, key: '' })); setError(t.common.uiUnableToLoad); },
  });
  const deleteMut = useMutation({ mutationFn: deleteClientKey, onSuccess: () => queryClient.invalidateQueries({ queryKey: ['client-keys'] }), onError: (e) => setError(apiError(e)) });
  const toggleMut = useMutation({
    mutationFn: ({ hash, enabled }: { hash: string; enabled: boolean }) => updateClientKey(hash, { enabled }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['client-keys'] }),
    onError: (e) => setError(apiError(e)),
  });
  const rotateMut = useMutation({
    mutationFn: () => rotateClientKey(rotateKey.hash, rotateKey.newKey),
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['client-keys'] }); closeRotate(); },
    onError: () => { setRotateKey((value) => ({ ...value, newKey: '' })); setError(t.common.uiUnableToLoad); },
  });

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div>
          <h1 className="text-xl font-semibold text-surface-900 sm:text-2xl">{t.clientKeys.title}</h1>
          <p className="text-sm text-surface-500 mt-1">{t.clientKeys.subtitle}</p>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-xs text-surface-400">{t.clientKeys.keysCount.replace('{n}', String(data?.total ?? 0))}</span>
          <button onClick={() => { clearNewKey(); setShowAdd(true); }} className="btn-gold flex items-center gap-1.5 text-sm"><Plus size={14} /> {t.clientKeys.addKey}</button>
        </div>
      </div>

      {error && <div className="rounded-lg bg-red-50 p-3 text-sm text-red-700">{error}</div>}

      {showAdd && (
        <div className="glass-card rounded-lg p-5 space-y-3 border-primary-200">
          <div className="flex items-center gap-2 mb-1"><Key size={14} className="text-primary-500" /><span className="text-sm font-medium text-surface-700">{t.clientKeys.newKey}</span></div>
          <p className="text-xs text-surface-500">{t.adminUi.secretHint}</p>
          <div className="flex flex-col gap-3 sm:flex-row">
            <input type="password" autoComplete="new-password" placeholder={t.clientKeys.apiKey} value={newKey.key} onChange={e => setNewKey(p => ({ ...p, key: e.target.value }))} className="input-glass flex-1" />
            <input placeholder={t.clientKeys.tenant} value={newKey.tenant} onChange={e => setNewKey(p => ({ ...p, tenant: e.target.value }))} className="input-glass w-36" />
            <input placeholder={t.clientKeys.label} value={newKey.label} onChange={e => setNewKey(p => ({ ...p, label: e.target.value }))} className="input-glass w-36" />
          </div>
          <div className="flex gap-2">
            <button onClick={() => addMut.mutate()} className="btn-primary text-xs px-5">{t.clientKeys.create}</button>
            <button onClick={closeAdd} className="px-4 py-2 rounded-lg text-xs text-surface-500 hover:text-surface-700 hover:bg-surface-50 transition-colors">{t.clientKeys.cancel}</button>
          </div>
        </div>
      )}

      {rotateKey.hash && (
        <div className="glass-card rounded-lg p-5 space-y-3 border-accent-200 bg-accent-50/30">
          <div className="flex items-center gap-2 mb-1"><RefreshCw size={14} className="text-accent-600" /><span className="text-sm font-medium text-accent-700">{t.clientKeys.rotateKey}: <span className="font-mono text-xs">{rotateKey.hash}</span></span></div>
          <p className="text-xs text-surface-500">{t.adminUi.secretHint}</p>
          <div className="flex flex-col gap-3 sm:flex-row">
            <input type="password" autoComplete="new-password" placeholder={t.clientKeys.newApiKey} value={rotateKey.newKey} onChange={e => setRotateKey(p => ({ ...p, newKey: e.target.value }))} className="input-glass flex-1" />
            <button onClick={() => rotateMut.mutate()} className="btn-gold text-xs">{t.clientKeys.rotate}</button>
            <button onClick={closeRotate} className="px-4 py-2 rounded-lg text-xs text-surface-500 hover:text-surface-700 hover:bg-surface-50 transition-colors">{t.clientKeys.cancel}</button>
          </div>
        </div>
      )}

      {isLoading ? <Skeleton className="h-64 w-full" /> : isError ? <ErrorState action={<Button size="sm" variant="secondary" onClick={() => refetch()}>{t.common.uiRetry}</Button>} /> : data?.keys.length === 0 ? <EmptyState title={t.clientKeys.noKeys} /> : <div className="glass-card rounded-lg overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-surface-100 bg-surface-50/50">
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.clientKeys.thHash}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.clientKeys.thTenant}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.clientKeys.thLabel}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.clientKeys.thStatus}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.clientKeys.thCreated}</th>
                <th className="text-left p-3 text-xs text-surface-400 uppercase font-semibold">{t.clientKeys.thActions}</th>
              </tr>
            </thead>
            <tbody>
              {data?.keys.map(k => (
                <tr key={k.key_hash} className="border-b border-surface-50 hover:bg-surface-50/50 transition-colors">
                  <td className="p-3 font-mono text-xs text-surface-500">{k.key_hash}</td>
                  <td className="p-3 text-surface-700 font-medium">{k.tenant_id}</td>
                  <td className="p-3 text-surface-500">{k.label || '—'}</td>
                  <td className="p-3"><span className={`badge ${k.enabled ? 'badge-success' : 'badge-error'}`}>{k.enabled ? t.clientKeys.enabled : t.clientKeys.disabled}</span></td>
                  <td className="p-3 text-surface-500 text-xs">{new Date(k.created_at).toLocaleDateString()}</td>
                  <td className="p-3">
                    <div className="flex items-center gap-1">
                      <button onClick={() => toggleMut.mutate({ hash: k.key_hash, enabled: !k.enabled })} className="p-2 rounded-lg hover:bg-surface-100 text-surface-400 hover:text-surface-600 transition-colors" title={k.enabled ? t.clientKeys.disabled : t.clientKeys.enabled}>{k.enabled ? <X size={14} /> : <Check size={14} />}</button>
                      <button onClick={() => setRotateKey({ hash: k.key_hash, newKey: '' })} className="p-2 rounded-lg hover:bg-surface-100 text-surface-400 hover:text-accent-600 transition-colors" title={t.clientKeys.rotate}><RefreshCw size={14} /></button>
                      <button onClick={() => { if (window.confirm(t.adminUi.confirmDelete)) deleteMut.mutate(k.key_hash); }} className="p-2 rounded-lg hover:bg-surface-100 text-surface-400 hover:text-red-600 transition-colors" title={t.adminUi.delete} aria-label={`${t.adminUi.delete} ${k.key_hash}`}><Trash2 size={14} /></button>
                    </div>
                  </td>
                </tr>
              ))}

            </tbody>
          </table>
        </div>
      </div>}
    </div>
  );
}
