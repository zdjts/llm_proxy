import { useState, FormEvent } from 'react';
import { useNavigate, Navigate } from 'react-router-dom';
import { motion } from 'framer-motion';
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

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError('');
    setLoading(true);
    try {
      await login(email, password);
      navigate('/', { replace: true });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Login failed';
      setError(msg);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-gradient-to-br from-surface-50 via-white to-primary-50/30 p-4">
      <div className="absolute inset-0 z-0">
        <div className="absolute top-1/4 left-1/4 w-96 h-96 bg-primary-400/10 rounded-full blur-3xl" />
        <div className="absolute bottom-1/4 right-1/4 w-96 h-96 bg-amber-400/10 rounded-full blur-3xl" />
      </div>

      <motion.div
        initial={{ opacity: 0, y: 30, scale: 0.97 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ duration: 0.4 }}
        className="relative z-10 w-full max-w-sm"
      >
        <div className="glass p-8 rounded-2xl border border-white/20 shadow-xl shadow-black/5">
          <div className="text-center mb-6">
            <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary-500 to-primary-700 flex items-center justify-center text-white font-bold text-lg mx-auto mb-3 shadow-md shadow-primary-500/20">
              P
            </div>
            <h1 className="text-xl font-bold text-surface-800">llm_proxy</h1>
            <p className="text-sm text-surface-400 mt-1">Sign in to continue</p>
          </div>

          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-surface-600 mb-1">Email</label>
              <input
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                className="w-full px-3 py-2.5 rounded-lg border border-surface-200 bg-white/50 text-sm focus:ring-2 focus:ring-primary-500/30 focus:border-primary-500 outline-none transition-all"
                placeholder="admin@example.com"
                required
                autoFocus
              />
            </div>
            <div>
              <label className="block text-sm font-medium text-surface-600 mb-1">Password</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="w-full px-3 py-2.5 rounded-lg border border-surface-200 bg-white/50 text-sm focus:ring-2 focus:ring-primary-500/30 focus:border-primary-500 outline-none transition-all"
                placeholder="••••••••"
                required
              />
            </div>

            {error && (
              <div className="flex items-center gap-2 text-red-600 bg-red-50 border border-red-200 rounded-lg px-3 py-2 text-sm">
                <AlertCircle size={14} />
                <span>{error}</span>
              </div>
            )}

            <button
              type="submit"
              disabled={loading}
              className="w-full py-2.5 bg-primary-600 hover:bg-primary-700 disabled:opacity-60 text-white rounded-lg text-sm font-medium transition-colors flex items-center justify-center gap-2"
            >
              {loading ? <Loader2 size={16} className="animate-spin" /> : <LogIn size={16} />}
              Sign In
            </button>
          </form>
        </div>

        <p className="text-center text-xs text-surface-300 mt-4">
          Access to this gateway requires valid credentials
        </p>
      </motion.div>
    </div>
  );
}
