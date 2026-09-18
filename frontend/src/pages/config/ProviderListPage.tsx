import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import { createProvider, deleteProvider, fetchProviders, apiError } from '@/lib/api';
import { Button, Modal, Badge, EmptyState, ErrorState, Skeleton, Table } from '@/components/ui';
import { useLocale } from '@/i18n/context';
import { useState } from 'react';

export function ProviderListPage() {
  const qc = useQueryClient();
  const { t } = useLocale();
  const { data, isLoading, isError, refetch } = useQuery({ queryKey: ['providers'], queryFn: fetchProviders });
  const [formError, setFormError] = useState('');
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState({ id: '', kind: 'openai', base_url: '', pool_id: '' });

  const createMutation = useMutation({
    mutationFn: () => createProvider(form),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['providers'] }); setShowCreate(false); },
    onError: (error) => setFormError(apiError(error)),
  });

  const deleteMutation = useMutation({
    mutationFn: deleteProvider,
    onSuccess: () => qc.invalidateQueries({ queryKey: ['providers'] }),
    onError: (error) => setFormError(apiError(error)),
  });

  const providers = data?.providers || [];

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div>
          <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.configAdmin.providerTitle}</h1>
          <p className="mt-1 text-[14.5px] text-surface-600">{t.configAdmin.providerSubtitle}</p>
        </div>
        <Button onClick={() => setShowCreate(true)}><Plus size={16} /> {t.configAdmin.addProvider}</Button>
      </div>

      {isLoading ? <Skeleton className="h-64 w-full" /> : isError ? <ErrorState action={<Button size="sm" variant="secondary" onClick={() => refetch()}>{t.adminUi.retry}</Button>} /> : providers.length === 0 ? (
        <EmptyState title={t.adminUi.noProviders} description={t.configAdmin.providerEmptyHint} action={<Button onClick={() => setShowCreate(true)}><Plus size={16} /> {t.configAdmin.addProvider}</Button>} />
      ) : (
        <Table
          headers={['ID', 'Kind', t.configAdmin.baseUrl, 'Pool', t.adminUi.actions]}
          rows={providers.map((p: Record<string, unknown>) => [
            <span className="font-mono text-xs">{p.id as string}</span>,
            <Badge variant="info">{p.kind as string}</Badge>,
            <span className="font-mono text-xs text-surface-500">{p.base_url as string}</span>,
            <span className="font-mono text-xs">{p.pool_id as string}</span>,
            <div className="flex items-center gap-2">
              <Button variant="danger" size="sm" loading={deleteMutation.isPending} aria-label={`${t.adminUi.delete} ${p.id as string}`} onClick={() => { if (window.confirm(t.adminUi.confirmDelete)) deleteMutation.mutate(p.id as string); }}><Trash2 size={14} /></Button>
            </div>,
          ])}
        />
      )}

      <Modal open={showCreate} onClose={() => { setShowCreate(false); setFormError(''); }} title={t.configAdmin.addProvider}>
        <div className="space-y-3">
          <input className="input-glass w-full" placeholder={t.configAdmin.providerId} value={form.id} onChange={e => setForm({ ...form, id: e.target.value })} />
          <select className="input-glass w-full" value={form.kind} onChange={e => setForm({ ...form, kind: e.target.value })}>
            <option value="openai">OpenAI</option><option value="anthropic">Anthropic</option><option value="gemini">Gemini</option>
          </select>
          <input className="input-glass w-full" placeholder={t.configAdmin.baseUrl} value={form.base_url} onChange={e => setForm({ ...form, base_url: e.target.value })} />
          <input className="input-glass w-full" placeholder={t.configAdmin.poolId} value={form.pool_id} onChange={e => setForm({ ...form, pool_id: e.target.value })} />
          {formError && <p className="text-sm text-danger-dark">{formError}</p>}
          <div className="flex gap-2 justify-end pt-2">
            <Button variant="secondary" onClick={() => setShowCreate(false)}>{t.adminUi.cancel}</Button>
            <Button onClick={() => createMutation.mutate()} loading={createMutation.isPending}>{t.adminUi.create}</Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
