import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Download, RefreshCw, ShieldCheck } from 'lucide-react';
import { apiError, downloadExport, fetchConfigOverview, importConfig, refreshConfig, validateConfig } from '@/lib/api';

export function ConfigConsolePage() {
  const qc = useQueryClient();
  const overview = useQuery({ queryKey: ['config-overview'], queryFn: fetchConfigOverview });
  const [yaml, setYaml] = useState('');
  const [message, setMessage] = useState('');
  const validate = useMutation({ mutationFn: () => validateConfig(yaml), onSuccess: (r) => setMessage(r.message), onError: (e) => setMessage(apiError(e)) });
  const dryRun = useMutation({ mutationFn: () => importConfig(yaml, true), onSuccess: (r) => setMessage(`Dry run: ${r.message}`), onError: (e) => setMessage(apiError(e)) });
  const commit = useMutation({ mutationFn: () => importConfig(yaml, false), onSuccess: (r) => { setMessage(`Imported; active version ${r.version}`); qc.invalidateQueries({ queryKey: ['config-overview'] }); }, onError: (e) => setMessage(apiError(e)) });
  const refresh = useMutation({ mutationFn: refreshConfig, onSuccess: (r) => { setMessage(`Refreshed DB snapshot; active version ${r.version}`); qc.invalidateQueries({ queryKey: ['config-overview'] }); }, onError: (e) => setMessage(apiError(e)) });
  const busy = validate.isPending || dryRun.isPending || commit.isPending || refresh.isPending;
  const run = (fn: () => void) => { if (yaml.trim()) fn(); else setMessage('Enter YAML before validating or importing.'); };
  return <div className="space-y-6">
    <header className="flex items-start justify-between gap-4"><div><h1 className="text-2xl font-bold text-surface-800">Configuration Console</h1><p className="text-sm text-surface-500 mt-1">Manage only SQLite-backed providers, pools, routing and registry data.</p></div><button className="btn-secondary flex items-center gap-2" onClick={() => downloadExport().catch((e) => setMessage(apiError(e)))}><Download size={15} /> Export DB config</button></header>
    <div className="grid grid-cols-2 md:grid-cols-5 gap-3">{[['Version', overview.data?.version ?? '—'], ['Providers', overview.data?.providers ?? '—'], ['Pools', overview.data?.pools ?? '—'], ['Routing', overview.data?.routing ?? '—'], ['Registry', overview.data?.model_registry ?? '—']].map(([label, value]) => <div className="glass-card p-4" key={label as string}><div className="text-xs uppercase text-surface-400">{label}</div><div className="text-2xl font-bold text-surface-800 mt-1">{value}</div></div>)}</div>
    <div className="glass-card p-5 space-y-4"><div className="flex items-center gap-2"><ShieldCheck size={16} className="text-emerald-600" /><h2 className="font-semibold">Explicit YAML validate / dry-run / import</h2></div><p className="text-xs text-surface-500">The browser sends this text to the admin API. It does not read server files or SQLite. Validate and dry-run do not modify the database.</p><textarea className="w-full min-h-72 rounded-xl border border-surface-200 p-3 font-mono text-xs" value={yaml} onChange={(e) => setYaml(e.target.value)} placeholder="Paste an explicit configuration document here…" /><div className="flex flex-wrap gap-2"><button className="btn-secondary" disabled={busy} onClick={() => run(() => validate.mutate())}>Validate</button><button className="btn-secondary" disabled={busy} onClick={() => run(() => dryRun.mutate())}>Dry run</button><button className="btn-primary" disabled={busy} onClick={() => run(() => { if (window.confirm('Commit this validated configuration to SQLite?')) commit.mutate(); })}>Commit import</button><button className="btn-secondary flex items-center gap-1" disabled={busy} onClick={() => refresh.mutate()}><RefreshCw size={14} /> Refresh DB</button></div>{message && <div className="rounded-lg bg-surface-50 p-3 text-sm text-surface-600">{message}</div>}</div>
    <div className="glass-card p-5"><h2 className="font-semibold mb-2">Not managed by this console</h2><p className="text-sm text-surface-500">Rate limits, concurrency, failover policy, alert policy, fallback, pipeline and cache policy remain startup/bootstrap-static because no DB carrier exists. Client authentication is bootstrap plus ephemeral runtime state; this console never displays plaintext keys.</p></div>
  </div>;
}
