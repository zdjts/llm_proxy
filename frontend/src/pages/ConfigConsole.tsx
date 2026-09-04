import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Download, RefreshCw, ShieldCheck } from 'lucide-react';
import { apiError, downloadExport, fetchConfigOverview, importConfig, refreshConfig, validateConfig } from '@/lib/api';
import { Button, Card, ErrorState, Skeleton } from '@/components/ui';
import { useLocale } from '@/i18n/context';

export function ConfigConsolePage() {
  const { t } = useLocale();
  const qc = useQueryClient();
  const overview = useQuery({ queryKey: ['config-overview'], queryFn: fetchConfigOverview });
  const [yaml, setYaml] = useState('');
  const [message, setMessage] = useState('');
  const [messageKind, setMessageKind] = useState<'success' | 'error'>('success');
  const reportError = (error: unknown) => { setMessageKind('error'); setMessage(apiError(error)); };
  const reportSuccess = (text: string) => { setMessageKind('success'); setMessage(text); };
  const validate = useMutation({ mutationFn: () => validateConfig(yaml), onSuccess: (r) => reportSuccess(r.message), onError: reportError });
  const dryRun = useMutation({ mutationFn: () => importConfig(yaml, true), onSuccess: (r) => reportSuccess(r.message), onError: reportError });
  const commit = useMutation({ mutationFn: () => importConfig(yaml, false), onSuccess: (r) => { reportSuccess(`${t.configAdmin.commitImport}: ${r.version}`); qc.invalidateQueries({ queryKey: ['config-overview'] }); }, onError: reportError });
  const refresh = useMutation({ mutationFn: refreshConfig, onSuccess: (r) => { reportSuccess(`${t.configAdmin.refresh}: ${r.version}`); qc.invalidateQueries({ queryKey: ['config-overview'] }); }, onError: reportError });
  const busy = validate.isPending || dryRun.isPending || commit.isPending || refresh.isPending;
  const run = (fn: () => void) => { if (yaml.trim()) fn(); else { setMessageKind('error'); setMessage(t.configAdmin.yamlRequired); } };
  const metrics = [[t.configAdmin.version, overview.data?.version ?? '-'], [t.configAdmin.providers, overview.data?.providers ?? '-'], [t.configAdmin.pools, overview.data?.pools ?? '-'], [t.configAdmin.routing, overview.data?.routing ?? '-'], [t.configAdmin.registry, overview.data?.model_registry ?? '-']];

  return <div className="space-y-6">
    <header className="flex border-b border-surface-200 pb-5 flex-col gap-3 sm:flex-row sm:items-end sm:justify-between"><div><h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.configAdmin.configTitle}</h1><p className="mt-1 text-[14.5px] text-surface-600">{t.configAdmin.configSubtitle}</p></div><Button variant="secondary" onClick={() => downloadExport().catch(reportError)}><Download size={15} /> {t.configAdmin.exportConfig}</Button></header>
    {overview.isLoading ? <Skeleton className="h-28 w-full" /> : overview.isError ? <ErrorState action={<Button size="sm" variant="secondary" onClick={() => overview.refetch()}>{t.adminUi.retry}</Button>} /> : <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-5">{metrics.map(([label, value]) => <Card className="p-4" key={label}><div className="text-xs font-medium text-surface-500">{label}</div><div className="mt-1 font-operational text-2xl font-bold tracking-tight text-surface-900">{value}</div></Card>)}</div>}
    <Card className="space-y-4 p-5"><div className="flex items-center gap-2"><ShieldCheck size={16} className="text-surface-800" /><h2 className="font-semibold text-surface-900">{t.configAdmin.validateImport}</h2></div><p className="text-xs text-surface-500">{t.configAdmin.configHint}</p><textarea className="input-glass min-h-72 w-full p-3 font-mono text-xs" value={yaml} onChange={(e) => setYaml(e.target.value)} placeholder={t.configAdmin.configPlaceholder} /><div className="flex flex-wrap gap-2"><Button variant="secondary" disabled={busy} onClick={() => run(() => validate.mutate())}>{t.configAdmin.validate}</Button><Button variant="secondary" disabled={busy} onClick={() => run(() => dryRun.mutate())}>{t.configAdmin.dryRun}</Button><Button disabled={busy} onClick={() => run(() => { if (window.confirm(t.configAdmin.confirmCommit)) commit.mutate(); })}>{t.configAdmin.commitImport}</Button><Button variant="secondary" disabled={busy} onClick={() => refresh.mutate()}><RefreshCw size={14} /> {t.configAdmin.refresh}</Button></div>{message && <div role="status" className={`rounded-md p-3 text-sm ${messageKind === 'error' ? 'bg-danger-light text-danger-dark' : 'bg-success-light text-success-dark'}`}>{message}</div>}</Card>
    <Card className="p-5"><h2 className="mb-2 font-semibold text-surface-900">{t.configAdmin.notManaged}</h2><p className="text-sm text-surface-500">{t.configAdmin.notManagedHint}</p></Card>
  </div>;
}
