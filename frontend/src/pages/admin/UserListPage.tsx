import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import { api, apiError } from '@/lib/api';
import { Button, Modal, Badge, EmptyState, ErrorState, Skeleton, Table } from '@/components/ui';
import { useLocale } from '@/i18n/context';
import { useState } from 'react';

export function UserListPage() {
  const { t } = useLocale();
  const qc = useQueryClient();
  const { data, isLoading, isError, refetch } = useQuery({ queryKey: ['users'], queryFn: () => api.get('/admin/api/users').then(r => r.data) });
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState({ email: '', name: '', password: '', role_ids: [] as string[] });
  const [error, setError] = useState('');
  const clearForm = () => setForm({ email: '', name: '', password: '', role_ids: [] });
  const createMut = useMutation({ mutationFn: () => api.post('/admin/api/users', form), onSuccess: () => { qc.invalidateQueries({ queryKey: ['users'] }); clearForm(); setShowCreate(false); }, onError: (e) => { setForm((value) => ({ ...value, password: '' })); setError(apiError(e)); } });
  const deleteMut = useMutation({ mutationFn: (id: string) => api.delete(`/admin/api/users/${id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ['users'] }), onError: (e) => setError(apiError(e)) });
  const users = data?.users || [];

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5"><div><h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.configAdmin.usersTitle}</h1><p className="mt-1 text-[14.5px] text-surface-600">{t.configAdmin.usersCount.replace('{count}', String(data?.total || 0))}</p></div><Button onClick={() => setShowCreate(true)}><Plus size={16} /> {t.configAdmin.addUser}</Button></div>
      {error && <ErrorState title={error} />}
      {isLoading ? <Skeleton className="h-64" /> : isError ? <ErrorState action={<Button size="sm" variant="secondary" onClick={() => refetch()}>{t.adminUi.retry}</Button>} /> : users.length === 0 ? <EmptyState title={t.adminUi.noUsers} /> : (
        <Table headers={[t.configAdmin.name, t.configAdmin.email, t.configAdmin.roles, t.adminUi.actions]} rows={users.map((u: Record<string, unknown>) => [
          u.name as string, <span className="font-mono text-xs">{u.email as string}</span>,
          <div className="flex gap-1">{(u.roles as string[]).map((r: string) => <Badge key={r} variant="info">{r}</Badge>)}</div>,
          <Button variant="danger" size="sm" loading={deleteMut.isPending} aria-label={`${t.adminUi.delete} ${u.email as string}`} onClick={() => { if (window.confirm(t.adminUi.confirmDelete)) deleteMut.mutate(u.id as string); }}><Trash2 size={14} /></Button>,
        ])} />
      )}
      <Modal open={showCreate} onClose={() => { clearForm(); setShowCreate(false); setError(''); }} title={t.configAdmin.createUser}>
        <div className="space-y-3">
          <input className="input-glass w-full" placeholder={t.configAdmin.name} value={form.name} onChange={e => setForm({ ...form, name: e.target.value })} />
          <input className="input-glass w-full" placeholder={t.configAdmin.email} value={form.email} onChange={e => setForm({ ...form, email: e.target.value })} />
          <input className="input-glass w-full" type="password" autoComplete="new-password" placeholder={t.configAdmin.password} value={form.password} onChange={e => setForm({ ...form, password: e.target.value })} />
          <p className="text-xs text-surface-500">{t.adminUi.secretHint}</p>
          <div className="flex gap-2 justify-end pt-2"><Button variant="secondary" onClick={() => { clearForm(); setShowCreate(false); }}>{t.adminUi.cancel}</Button><Button onClick={() => createMut.mutate()} loading={createMut.isPending}>{t.adminUi.create}</Button></div>
        </div>
      </Modal>
    </div>
  );
}
