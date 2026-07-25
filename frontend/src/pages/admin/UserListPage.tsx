import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import { api } from '@/lib/api';
import { Card, Button, Modal, Badge, EmptyState, Skeleton, Table } from '@/components/ui';
import { useState } from 'react';

export function UserListPage() {
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ['users'], queryFn: () => api.get('/admin/api/users').then(r => r.data) });
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState({ email: '', name: '', password: '', role_ids: [] as string[] });
  const createMut = useMutation({ mutationFn: () => api.post('/admin/api/users', form), onSuccess: () => { qc.invalidateQueries({ queryKey: ['users'] }); setShowCreate(false); } });
  const deleteMut = useMutation({ mutationFn: (id: string) => api.delete(`/admin/api/users/${id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ['users'] }) });

  if (isLoading) return <Skeleton className="h-64" />;
  const users = data?.users || [];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between"><div><h1 className="text-2xl font-bold text-surface-800">Users</h1><p className="text-sm text-surface-400 mt-1">{data?.total || 0} users</p></div><Button onClick={() => setShowCreate(true)}><Plus size={16} /> Add User</Button></div>
      {users.length === 0 ? <EmptyState title="No users" /> : (
        <Table headers={['Name', 'Email', 'Roles', 'Actions']} rows={users.map((u: Record<string, unknown>) => [
          u.name as string, <span className="font-mono text-xs">{u.email as string}</span>,
          <div className="flex gap-1">{(u.roles as string[]).map((r: string) => <Badge key={r} variant="info">{r}</Badge>)}</div>,
          <Button variant="ghost" size="sm" onClick={() => deleteMut.mutate(u.id as string)}>🗑</Button>,
        ])} />
      )}
      <Modal open={showCreate} onClose={() => setShowCreate(false)} title="Create User">
        <div className="space-y-3">
          <input className="w-full px-3 py-2 rounded-lg border border-surface-200 text-sm" placeholder="Name" value={form.name} onChange={e => setForm({ ...form, name: e.target.value })} />
          <input className="w-full px-3 py-2 rounded-lg border border-surface-200 text-sm" placeholder="Email" value={form.email} onChange={e => setForm({ ...form, email: e.target.value })} />
          <input className="w-full px-3 py-2 rounded-lg border border-surface-200 text-sm" type="password" placeholder="Password" value={form.password} onChange={e => setForm({ ...form, password: e.target.value })} />
          <div className="flex gap-2 justify-end pt-2"><Button variant="secondary" onClick={() => setShowCreate(false)}>Cancel</Button><Button onClick={() => createMut.mutate()} loading={createMut.isPending}>Create</Button></div>
        </div>
      </Modal>
    </div>
  );
}
