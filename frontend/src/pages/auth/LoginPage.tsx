import { useState, FormEvent } from 'react';
import { useNavigate, Navigate } from 'react-router-dom';
import { LogIn, AlertCircle, Loader2 } from 'lucide-react';
import { useAuthStore } from '@/stores/authStore';

export function LoginPage() {
  const { login, isAuthenticated } = useAuthStore();
  const navigate = useNavigate();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  if (isAuthenticated) return <Navigate to="/" replace />;

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setError('');
    setLoading(true);
    try { await login(email, password); navigate('/console', { replace: true }); }
    catch (err: unknown) { setError(err instanceof Error ? err.message : 'Login failed'); }
    finally { setLoading(false); }
  }

  return <main className="flex min-h-screen items-center justify-center bg-surface-50 p-4">
    <div className="w-full max-w-sm rounded-xl border border-surface-200 bg-white p-6 shadow-sm sm:p-8">
      <div className="mb-7 text-center">
        <div className="mx-auto mb-3 flex h-10 w-10 items-center justify-center rounded-md bg-primary-700 font-mono text-base font-semibold text-white">P</div>
        <h1 className="text-xl font-semibold text-surface-900">llm_proxy</h1>
        <p className="mt-1 text-sm text-surface-500">Sign in to continue</p>
      </div>
      <form onSubmit={handleSubmit} className="space-y-4">
        <div><label className="mb-1 block text-sm font-medium text-surface-700">Email</label><input type="email" value={email} onChange={(event) => setEmail(event.target.value)} className="input-glass w-full" placeholder="admin@example.com" required autoFocus /></div>
        <div><label className="mb-1 block text-sm font-medium text-surface-700">Password</label><input type="password" value={password} onChange={(event) => setPassword(event.target.value)} className="input-glass w-full" placeholder="••••••••" required /></div>
        {error && <div className="flex items-center gap-2 rounded-md border border-danger-light bg-primary-50 px-3 py-2 text-sm text-danger-dark"><AlertCircle size={14} aria-hidden="true" /><span>{error}</span></div>}
        <button type="submit" disabled={loading} className="btn-primary w-full">{loading ? <Loader2 size={16} className="animate-spin" aria-hidden="true" /> : <LogIn size={16} aria-hidden="true" />}Sign In</button>
      </form>
    </div>
    <p className="sr-only">Access to this gateway requires valid credentials</p>
  </main>;
}
