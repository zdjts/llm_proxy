import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import {
  apiError,
  createRouting,
  deleteRouting,
  fetchPools,
  fetchRouting,
} from '@/lib/api';
import { Badge, Button, Card, EmptyState, ErrorState, Modal, Skeleton } from '@/components/ui';
import { useLocale } from '@/i18n/context';

export function RoutingConfigPage() {
  const { t } = useLocale();
  const qc = useQueryClient();
  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['routing'],
    queryFn: fetchRouting,
  });
  const poolsQuery = useQuery({ queryKey: ['pools'], queryFn: fetchPools });
  const [open, setOpen] = useState(false);
  const [logicalModel, setLogicalModel] = useState('');
  const [poolId, setPoolId] = useState('');
  const [error, setError] = useState('');

  const createMutation = useMutation({
    mutationFn: () => createRouting({ logical_model: logicalModel.trim(), pool_id: poolId }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['routing'] });
      setOpen(false);
      setLogicalModel('');
      setPoolId('');
      setError('');
    },
    onError: (mutationError) => setError(apiError(mutationError)),
  });
  const deleteMutation = useMutation({
    mutationFn: (model: string) => deleteRouting(model),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['routing'] }),
    onError: (mutationError) => setError(apiError(mutationError)),
  });

  const entries = data?.routing || [];
  const pools = poolsQuery.data?.pools || [];

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-3 border-b border-surface-200 pb-5 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">
            {t.configAdmin.routingTitle}
          </h1>
          <p className="mt-1 text-[14.5px] text-surface-600">{t.configAdmin.routingSubtitle}</p>
        </div>
        <Button
          onClick={() => {
            setOpen(true);
            setError('');
            setPoolId(pools[0]?.id ?? '');
          }}
        >
          <Plus size={16} /> {t.adminUi.add}
        </Button>
      </div>
      {isLoading ? (
        <Skeleton className="h-64" />
      ) : isError ? (
        <ErrorState
          action={
            <Button size="sm" variant="secondary" onClick={() => refetch()}>
              {t.adminUi.retry}
            </Button>
          }
        />
      ) : entries.length === 0 ? (
        <EmptyState title={t.adminUi.noRouting} />
      ) : (
        <div className="grid gap-3">
          {entries.map((e: { logical_model: string; pool_id: string }) => (
            <Card key={e.logical_model} className="flex items-center justify-between gap-3 px-5 py-4">
              <span className="min-w-0 truncate font-mono text-sm font-semibold">
                {e.logical_model}
              </span>
              <span className="text-sm text-surface-400" aria-hidden="true">
                →
              </span>
              <Badge variant="info">{e.pool_id}</Badge>
              <Button
                size="sm"
                variant="secondary"
                loading={deleteMutation.isPending}
                onClick={() => deleteMutation.mutate(e.logical_model)}
              >
                <Trash2 size={14} />
              </Button>
            </Card>
          ))}
        </div>
      )}
      {error && !open ? <p className="text-sm text-danger-dark">{error}</p> : null}
      <Modal
        open={open}
        onClose={() => {
          setOpen(false);
          setError('');
        }}
        title={t.configAdmin.routingTitle}
      >
        <div className="space-y-3">
          <input
            className="input-glass w-full"
            placeholder="logical_model"
            value={logicalModel}
            onChange={(event) => setLogicalModel(event.target.value)}
          />
          <select
            className="input-glass w-full"
            value={poolId}
            onChange={(event) => setPoolId(event.target.value)}
          >
            <option value="">{t.configAdmin.poolId}</option>
            {pools.map((pool) => (
              <option key={pool.id} value={pool.id}>
                {pool.id}
              </option>
            ))}
          </select>
          {error ? <p className="text-sm text-danger-dark">{error}</p> : null}
          <div className="flex justify-end">
            <Button
              loading={createMutation.isPending}
              disabled={!logicalModel.trim() || !poolId}
              onClick={() => createMutation.mutate()}
            >
              {t.adminUi.save}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
