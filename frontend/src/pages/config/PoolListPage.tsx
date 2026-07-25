import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import { api } from '@/lib/api';
import { Card, Button, Modal, Badge, EmptyState, Skeleton, Table, StatusDot } from '@/components/ui';
import { useState } from 'react';

export function PoolListPage() {
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ['pools'], queryFn: () => api.get('/admin/api/pools').then(r => r.data) });
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState({ id: '', strategy: 'weighted_random', keys: [{ key: '', weight: 1 }] });

  const createMut = useMutation({
    mutationFn: () => api.post('/admin/api/pools', form),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['pools'] }); setShowCreate(false); },
  });
  const deleteMut = useMutation({
    mutationFn: (id: string) => api.delete(`/admin/api/pools/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['pools'] }),
  });

  if (isLoading) return <Skeleton className="h-64 w-full" />;
  const pools = data?.pools || [];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div><h1 className="text-2xl font-bold text-surface-800">Key Pools</h1><p className="text-sm text-surface-400 mt-1">Manage upstream API key pools</p></div>
        <Button onClick={() => setShowCreate(true)}><Plus size={16} /> Add Pool</Button>
      </div>
      {pools.length === 0 ? <EmptyState title="No key pools" /> : (
        <div className="space-y-4">
          {pools.map((p: Record<string, unknown>) => (
            <Card key={p.id as string} className="p-5">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <h3 className="font-mono text-sm font-semibold">{p.id as string}</h3>
                  <Badge variant="info">{p.strategy as string}</Badge>
                  <Badge>{(p.key_count as number)} keys</Badge>
                </div>
                <Button variant="ghost" size="sm" onClick={() => deleteMut.mutate(p.id as string)}><Trash2 size={14} className="text-red-400" /></Button>
              </div>
              <Table
                headers={['Key Hash', 'Weight', 'Status']}
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
      <Modal open={showCreate} onClose={() => setShowCreate(false)} title="Add Key Pool">
        <div className="space-y-3">
          <input className="w-full px-3 py-2 rounded-lg border border-surface-200 text-sm" placeholder="Pool ID" value={form.id} onChange={e => setForm({ ...form, id: e.target.value })} />
          {form.keys.map((k, i) => (
            <div key={i} className="flex gap-2">
              <input className="flex-1 px-3 py-2 rounded-lg border border-surface-200 text-sm" placeholder="API Key" value={k.key} onChange={e => { const keys = [...form.keys]; keys[i].key = e.target.value; setForm({ ...form, keys }); }} />
              <input className="w-20 px-3 py-2 rounded-lg border border-surface-200 text-sm" type="number" min="1" value={k.weight} onChange={e => { const keys = [...form.keys]; keys[i].weight = parseInt(e.target.value) || 1; setForm({ ...form, keys }); }} />
            </div>
          ))}
          <Button variant="secondary" size="sm" onClick={() => setForm({ ...form, keys: [...form.keys, { key: '', weight: 1 }] })}>+ Add Key</Button>
          <div className="flex gap-2 justify-end pt-2"><Button variant="secondary" onClick={() => setShowCreate(false)}>Cancel</Button><Button onClick={() => createMut.mutate()} loading={createMut.isPending}>Create</Button></div>
        </div>
      </Modal>
    </div>
  );
}
