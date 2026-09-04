import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import {
  apiError,
  createModel,
  fetchModels,
  fetchProviders,
  type ModelRegistry,
  updateModel,
} from '@/lib/api';
import { Badge, Button, Card, EmptyState, ErrorState, Modal, Skeleton } from '@/components/ui';
import { useLocale } from '@/i18n/context';

const blank = (): ModelRegistry => ({
  id: '',
  display_name: '',
  provider_kind: 'openai',
  provider_config_id: null,
  supports_vision: false,
  supports_tool_calling: false,
  supports_json_mode: false,
  max_context_tokens: 4096,
  max_output_tokens: 4096,
  input_price_per_1m: null,
  output_price_per_1m: null,
  capabilities_json: {},
  enabled: true,
});

export function ModelCatalogPage() {
  const { t } = useLocale();
  const qc = useQueryClient();
  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['model-catalog'],
    queryFn: fetchModels,
  });
  const providersQuery = useQuery({ queryKey: ['providers'], queryFn: fetchProviders });
  const [editing, setEditing] = useState<ModelRegistry | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [error, setError] = useState('');
  const mutation = useMutation({
    mutationFn: (model: ModelRegistry) => (isNew ? createModel(model) : updateModel(model.id, model)),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['model-catalog'] });
      qc.invalidateQueries({ queryKey: ['routing'] });
      setEditing(null);
      setError('');
    },
    onError: (mutationError) => setError(apiError(mutationError)),
  });
  const models = data?.models ?? [];
  const providers = providersQuery.data?.providers ?? [];
  const openNew = () => {
    setIsNew(true);
    setEditing({
      ...blank(),
      provider_config_id: providers[0]?.id ?? null,
      provider_kind: providers[0]?.kind ?? 'openai',
    });
    setError('');
  };
  const openEdit = (model: ModelRegistry) => {
    setIsNew(false);
    setEditing({ ...model });
    setError('');
  };

  return (
    <div className="space-y-6">
      <header className="flex flex-col gap-3 border-b border-surface-200 pb-5 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">
            {t.configAdmin.modelTitle}
          </h1>
          <p className="mt-1 text-[14.5px] text-surface-600">{t.configAdmin.modelSubtitle}</p>
        </div>
        <Button onClick={openNew}>
          <Plus size={16} /> {t.configAdmin.newModel}
        </Button>
      </header>
      {isLoading ? (
        <Skeleton className="h-64 w-full" />
      ) : isError ? (
        <ErrorState
          action={
            <Button size="sm" variant="secondary" onClick={() => refetch()}>
              {t.adminUi.retry}
            </Button>
          }
        />
      ) : models.length === 0 ? (
        <EmptyState title={t.adminUi.noModels} />
      ) : (
        <div className="grid gap-3 md:grid-cols-2">
          {models.map((model) => (
            <Card key={model.id} className="p-5">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="truncate font-semibold text-surface-900">{model.display_name}</div>
                  <div className="mt-1 truncate font-mono text-xs text-surface-500">{model.id}</div>
                </div>
                <Badge variant={model.enabled ? 'success' : 'warning'}>
                  {model.enabled ? t.configAdmin.enabled : t.configAdmin.disabled}
                </Badge>
              </div>
              <div className="mt-3 text-xs text-surface-500">
                {model.provider_kind}
                {model.provider_config_id ? ` / ${model.provider_config_id}` : ''} |{' '}
                {t.configAdmin.context} {model.max_context_tokens} | {t.configAdmin.output}{' '}
                {model.max_output_tokens}
              </div>
              <div className="mt-1 text-xs text-surface-500">
                {t.configAdmin.vision} {model.supports_vision ? t.configAdmin.yes : t.configAdmin.no} |{' '}
                {t.configAdmin.tools}{' '}
                {model.supports_tool_calling ? t.configAdmin.yes : t.configAdmin.no} |{' '}
                {t.configAdmin.jsonMode} {model.supports_json_mode ? t.configAdmin.yes : t.configAdmin.no}
              </div>
              <Button className="mt-4" size="sm" variant="secondary" onClick={() => openEdit(model)}>
                {t.adminUi.edit}
              </Button>
            </Card>
          ))}
        </div>
      )}
      <Modal
        open={editing !== null}
        onClose={() => {
          setEditing(null);
          setError('');
        }}
        title={isNew ? t.configAdmin.newModel : t.adminUi.edit}
      >
        {editing && (
          <div className="space-y-3">
            {(['id', 'display_name', 'provider_kind'] as const).map((key) => (
              <input
                key={key}
                disabled={key === 'id' && !isNew}
                className="input-glass w-full"
                placeholder={key}
                value={editing[key] ?? ''}
                onChange={(event) => setEditing({ ...editing, [key]: event.target.value })}
              />
            ))}
            <select
              className="input-glass w-full"
              value={editing.provider_config_id ?? ''}
              onChange={(event) => {
                const provider = providers.find((item) => item.id === event.target.value);
                setEditing({
                  ...editing,
                  provider_config_id: event.target.value || null,
                  provider_kind: provider?.kind ?? editing.provider_kind,
                });
              }}
            >
              <option value="">{t.configAdmin.providerId}</option>
              {providers.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.id} ({provider.kind} → {provider.pool_id})
                </option>
              ))}
            </select>
            {(['max_context_tokens', 'max_output_tokens', 'input_price_per_1m', 'output_price_per_1m'] as const).map(
              (key) => (
                <input
                  key={key}
                  type="number"
                  min={key.includes('price') ? 0 : 1}
                  className="input-glass w-full"
                  placeholder={key}
                  value={editing[key] ?? ''}
                  onChange={(event) =>
                    setEditing({
                      ...editing,
                      [key]: event.target.value === '' ? null : Number(event.target.value),
                    })
                  }
                />
              ),
            )}
            <textarea
              className="input-glass w-full font-mono text-xs"
              placeholder="capabilities_json"
              value={JSON.stringify(editing.capabilities_json)}
              onChange={(event) => {
                try {
                  setEditing({ ...editing, capabilities_json: JSON.parse(event.target.value) });
                } catch {
                  /* Server validates malformed JSON. */
                }
              }}
            />
            <label className="flex gap-2 text-sm">
              <input
                type="checkbox"
                checked={editing.enabled}
                onChange={(event) => setEditing({ ...editing, enabled: event.target.checked })}
              />{' '}
              {t.configAdmin.enabled}
            </label>
            {(['supports_vision', 'supports_tool_calling', 'supports_json_mode'] as const).map((key) => (
              <label key={key} className="flex gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={editing[key]}
                  onChange={(event) => setEditing({ ...editing, [key]: event.target.checked })}
                />{' '}
                {key}
              </label>
            ))}
            {error && <p className="text-sm text-danger-dark">{error}</p>}
            <div className="flex justify-end">
              <Button loading={mutation.isPending} onClick={() => mutation.mutate(editing)}>
                {t.adminUi.save}
              </Button>
            </div>
          </div>
        )}
      </Modal>
    </div>
  );
}
