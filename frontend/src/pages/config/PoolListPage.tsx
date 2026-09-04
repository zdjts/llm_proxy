import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import { createPool, deletePool, fetchPools, apiError } from '@/lib/api';
import { Card, Button, Modal, Badge, EmptyState, ErrorState, Skeleton, Table, StatusDot } from '@/components/ui';
import { useLocale } from '@/i18n/context';
import { useState } from 'react';

const blankForm = () => ({ id: '', strategy: 'weighted_random', keys: [{ key: '', weight: 1 }] });

export function PoolListPage() {
  const qc = useQueryClient();
  const { t } = useLocale();
  const { data, isLoading, isError, refetch } = useQuery({ queryKey: ['pools'], queryFn: fetchPools });
  const [formError, setFormError] = useState('');
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState(blankForm);
  const clearCreateForm = () => { setForm(blankForm()); setFormError(''); };
  const closeCreate = () => { clearCreateForm(); setShowCreate(false); };
  const openCreate = () => { clearCreateForm(); setShowCreate(true); };

  const createMut = useMutation({
    mutationFn: () => createPool(form),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['pools'] }); closeCreate(); },
    onError: (error) => { clearCreateForm(); setFormError(apiError(error)); },
  });
  const deleteMut = useMutation({
    mutationFn: deletePool,
    onSuccess: () => qc.invalidateQueries({ queryKey: ['pools'] }),
    onError: (error) => setFormError(apiError(error)),
  });

  const pools = data?.pools || [];

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5">
        <div><h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.configAdmin.keyPoolsTitle}</h1><p className="mt-1 text-[14.5px] text-surface-600">{t.configAdmin.keyPoolsSubtitle}</p></div>
        <Button onClick={openCreate}><Plus size={16} /> {t.configAdmin.addPool}</Button>
      </div>
      {isLoading ? <Skeleton className="h-64 w-full" /> : isError ? <ErrorState action={<Button size="sm" variant="secondary" onClick={() => refetch()}>{t.adminUi.retry}</Button>} /> : pools.length === 0 ? <EmptyState title={t.adminUi.noPools} /> : (
        <div className="space-y-4">
          {pools.map((p: Record<string, unknown>) => (
            <Card key={p.id as string} className="p-5">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <h3 className="font-mono text-sm font-semibold">{p.id as string}</h3>
                  <Badge variant="info">{p.strategy as string}</Badge>
                  <Badge>{(p.key_count as number)} keys</Badge>
                </div>
                <Button variant="danger" size="sm" loading={deleteMut.isPending} aria-label={`${t.adminUi.delete} ${p.id as string}`} onClick={() => { if (window.confirm(t.adminUi.confirmDelete)) deleteMut.mutate(p.id as string); }}><Trash2 size={14} /></Button>
              </div>
              <Table
                headers={[t.configAdmin.keyHash, t.configAdmin.weight, t.configAdmin.status]}
                rows={(p.keys as Array<{ key_hash: string; weight: number }>).map(k => [
                  <span className="font-mono text-xs">{k.key_hash}</span>,
                  k.weight.toString(),
                  <StatusDot status="healthy" />,
                ])}
              />
            </Card>
          ))}
        </div>
      )}
      <Modal open={showCreate} onClose={closeCreate} title={t.configAdmin.addPool}>
        <div className="space-y-3">
          <input className="input-glass w-full" placeholder={t.configAdmin.poolId} value={form.id} onChange={e => setForm({ ...form, id: e.target.value })} />
          {form.keys.map((k, i) => (
            <div key={i} className="flex gap-2">
              <input className="input-glass flex-1" type="password" autoComplete="new-password" placeholder={t.configAdmin.apiKey} value={k.key} onChange={e => { const keys = [...form.keys]; keys[i].key = e.target.value; setForm({ ...form, keys }); }} />
              <input className="input-glass w-20" type="number" min="1" value={k.weight} onChange={e => { const keys = [...form.keys]; keys[i].weight = parseInt(e.target.value) || 1; setForm({ ...form, keys }); }} />
            </div>
          ))}
          <Button variant="secondary" size="sm" onClick={() => setForm({ ...form, keys: [...form.keys, { key: '', weight: 1 }] })}>+ {t.configAdmin.addKey}</Button>
          {formError && <p className="text-sm text-danger-dark">{formError}</p>}
          <div className="flex gap-2 justify-end pt-2"><Button variant="secondary" onClick={closeCreate}>{t.adminUi.cancel}</Button><Button onClick={() => createMut.mutate()} loading={createMut.isPending}>{t.adminUi.create}</Button></div>
        </div>
      </Modal>
    </div>
  );
}
