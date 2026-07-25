import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2, RefreshCw, Server } from 'lucide-react';
import { api } from '@/lib/api';
import { Card, Button, Modal, Badge, EmptyState, Skeleton, Table } from '@/components/ui';
import { useState } from 'react';

export function ProviderListPage() {
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ['providers'], queryFn: () => api.get('/admin/api/providers').then(r => r.data) });
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState({ id: '', kind: 'openai', base_url: '', pool_id: '' });

  const createMutation = useMutation({
    mutationFn: () => api.post('/admin/api/providers', form),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['providers'] }); setShowCreate(false); },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.delete(`/admin/api/providers/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['providers'] }),
  });

  if (isLoading) return <div className="space-y-4"><Skeleton className="h-8 w-48" /><Skeleton className="h-64 w-full" /></div>;

  const providers = data?.providers || [];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-surface-800">Providers</h1>
          <p className="text-sm text-surface-400 mt-1">Manage upstream provider configurations</p>
        </div>
        <Button onClick={() => setShowCreate(true)}><Plus size={16} /> Add Provider</Button>
      </div>

      {providers.length === 0 ? (
        <EmptyState title="No providers configured" description="Add your first upstream provider to start routing requests." action={<Button onClick={() => setShowCreate(true)}><Plus size={16} /> Add Provider</Button>} />
      ) : (
        <Table
          headers={['ID', 'Kind', 'Base URL', 'Pool', 'Actions']}
          rows={providers.map((p: Record<string, unknown>) => [
            <span className="font-mono text-xs">{p.id as string}</span>,
            <Badge variant="info">{p.kind as string}</Badge>,
            <span className="font-mono text-xs text-surface-500">{p.base_url as string}</span>,
            <span className="font-mono text-xs">{p.pool_id as string}</span>,
            <div className="flex items-center gap-2">
              <Button variant="ghost" size="sm" onClick={() => deleteMutation.mutate(p.id as string)}><Trash2 size={14} className="text-red-400" /></Button>
            </div>,
          ])}
        />
      )}

      <Modal open={showCreate} onClose={() => setShowCreate(false)} title="Add Provider">
        <div className="space-y-3">
          <input className="w-full px-3 py-2 rounded-lg border border-surface-200 text-sm" placeholder="Provider ID (e.g. openai-east)" value={form.id} onChange={e => setForm({ ...form, id: e.target.value })} />
          <select className="w-full px-3 py-2 rounded-lg border border-surface-200 text-sm" value={form.kind} onChange={e => setForm({ ...form, kind: e.target.value })}>
            <option value="openai">OpenAI</option><option value="anthropic">Anthropic</option><option value="gemini">Gemini</option>
            <option value="azure">Azure</option><option value="cohere">Cohere</option><option value="mistral">Mistral</option>
            <option value="ollama">Ollama</option><option value="vllm">vLLM</option>
          </select>
          <input className="w-full px-3 py-2 rounded-lg border border-surface-200 text-sm" placeholder="Base URL (e.g. https://api.openai.com/v1)" value={form.base_url} onChange={e => setForm({ ...form, base_url: e.target.value })} />
          <input className="w-full px-3 py-2 rounded-lg border border-surface-200 text-sm" placeholder="Pool ID" value={form.pool_id} onChange={e => setForm({ ...form, pool_id: e.target.value })} />
          <div className="flex gap-2 justify-end pt-2">
            <Button variant="secondary" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button onClick={() => createMutation.mutate()} loading={createMutation.isPending}>Create</Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
