import { useState, FormEvent } from 'react';
import { useNavigate, Navigate, Link } from 'react-router-dom';
import { AlertCircle, ArrowLeft, Eye, EyeOff, Lock, LogIn, Mail, MessageCircle, Loader2, ShieldCheck, Timer, Zap } from 'lucide-react';
import { useAuthStore } from '@/stores/authStore';
import { apiError } from '@/lib/api';

export function LoginPage() {
  const { login, isAuthenticated } = useAuthStore();
  const navigate = useNavigate();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [showPassword, setShowPassword] = useState(false);

  if (isAuthenticated) return <Navigate to="/console" replace />;

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setError('');
    setLoading(true);
    try { await login(email, password); navigate('/console', { replace: true }); }
    catch (err: unknown) { setError(apiError(err)); }
    finally { setLoading(false); }
  }

  return <main className="auth-page min-h-screen bg-[#f7f6f2] text-surface-900">
    <header className="auth-header px-6 pt-[22px] sm:px-10"><div className="mx-auto flex max-w-[1280px] items-center justify-between border-b border-transparent pb-[14px]">
      <Link to="/" className="flex items-center gap-2.5 text-surface-900"><span className="grid h-[30px] w-[30px] place-items-center font-serif text-xl font-bold">P</span><span className="font-serif text-lg font-bold tracking-[0.06em]">llm_proxy</span></Link>
      <nav className="flex items-center gap-[18px]"><button type="button" title="English" className="inline-flex items-center gap-1.5 rounded-lg px-2 py-1.5 text-sm font-medium text-surface-600 hover:bg-surface-100"><span className="text-base">EN</span></button><Link to="/" className="inline-flex items-center gap-1.5 text-[13px] text-surface-600 hover:text-surface-900"><ArrowLeft size={15} />Back home</Link></nav>
    </div></header>
    <main className="flex px-6 pb-12 pt-8 sm:px-10 sm:pb-16 sm:pt-12"><div className="mx-auto grid w-full max-w-[1200px] items-center gap-12 lg:grid-cols-[1fr_480px] lg:gap-16">
      <aside className="hidden flex-col gap-7 pr-8 lg:flex" aria-hidden="true">
        <div className="relative mb-1 h-[118px] w-full max-w-[600px] overflow-hidden"><div className="absolute left-0 right-[8%] top-[45px] h-px bg-surface-800" /><span className="absolute left-[34%] top-[15px] h-[60px] w-[7px] skew-x-[-8deg] border-x border-surface-700 bg-white/20" /><span className="absolute left-[47%] top-1 h-[82px] w-[7px] skew-x-[-8deg] border-x border-surface-700 bg-white/20" /><span className="absolute left-[60%] top-[15px] h-[60px] w-[7px] skew-x-[-8deg] border-x border-surface-700 bg-white/20" /><span className="absolute right-[8%] top-[21px] h-12 w-[34px] rounded border border-surface-700 bg-white/40"><i className="absolute left-1 right-1 top-4 border-t border-surface-700" /><i className="absolute left-1 right-1 top-8 border-t border-surface-700" /><i className="absolute right-1 top-1.5 h-1 w-1 rounded-full bg-surface-800" /></span><div className="absolute left-0 top-[44px] flex animate-[pulse_4s_ease-in-out_infinite] gap-1"><i className="h-0.5 w-3.5 bg-surface-800" /><i className="h-0.5 w-1.5 bg-surface-800" /><i className="h-0.5 w-2.5 bg-surface-800" /><i className="h-0.5 w-1 bg-surface-800" /></div><span className="absolute left-1 top-[95px] text-[11px] tracking-[0.06em] text-surface-600">Your prompt</span><span className="absolute left-[47%] top-[95px] -ml-8 whitespace-nowrap text-[11px] tracking-[0.06em] text-surface-600">Byte-level passthrough</span><span className="absolute right-[4%] top-[95px] text-[11px] tracking-[0.06em] text-surface-600">Official upstream</span></div>
        <p className="font-operational text-[11px] uppercase tracking-[0.22em] text-surface-400">Privacy · Transparent · One thought</p>
        <h1 className="flex flex-col gap-2 font-serif text-[56px] font-bold leading-[1.15]"><span>One thought</span><span className="font-normal text-surface-600">Every model</span></h1>
        <ul className="space-y-[15px] border-t border-surface-200 pt-6 text-sm text-surface-700"><li className="flex items-center gap-3"><ShieldCheck size={18} strokeWidth={1.4} />No prompt storage · Never used for training</li><li className="flex items-center gap-3"><Zap size={18} strokeWidth={1.4} />End-to-end TLS · Fully auditable</li><li className="flex items-center gap-3"><Timer size={18} strokeWidth={1.4} />Self-serve · Live in seconds</li></ul>
      </aside>
      <section className="flex flex-col gap-5"><div className="rounded-[18px] border border-surface-200 bg-[linear-gradient(180deg,#fff,#fefdfa_55%,#fbf9f4)] px-8 py-9 shadow-[0_30px_60px_-28px_rgba(0,0,0,0.08)] sm:px-10 sm:py-11">
        <div className="mb-7 flex flex-col items-center gap-2.5 text-center"><div className="grid h-[52px] w-[52px] place-items-center rounded-[14px] border border-surface-200 font-serif text-2xl font-bold">P</div><h2 className="mt-1 font-serif text-[26px] font-bold tracking-[0.04em]">llm_proxy</h2><p className="font-serif text-[13px] italic text-surface-600">AI Gateway Console</p></div>
        <div className="space-y-6"><div className="text-center"><h3 className="font-sans text-2xl font-bold text-surface-900">Welcome Back</h3><p className="mt-2 text-sm text-surface-500">Sign in to your account to continue</p></div><form onSubmit={handleSubmit} className="space-y-5">
          <div><label className="mb-2 block text-[13px] font-medium tracking-[0.02em] text-surface-900">Email</label><div className="relative"><Mail className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-surface-400" size={20} strokeWidth={1.5} /><input type="email" value={email} onChange={(event) => setEmail(event.target.value)} className="input-glass h-11 w-full pl-11" placeholder="admin@example.com" required autoFocus /></div></div>
          <div><label className="mb-2 block text-[13px] font-medium tracking-[0.02em] text-surface-900">Password</label><div className="relative"><Lock className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-surface-400" size={20} strokeWidth={1.5} /><input type={showPassword ? 'text' : 'password'} value={password} onChange={(event) => setPassword(event.target.value)} className="input-glass h-11 w-full pl-11 pr-11" placeholder="••••••••" required /><button type="button" onClick={() => setShowPassword((value) => !value)} className="absolute inset-y-0 right-0 flex items-center pr-3.5 text-surface-400 hover:text-surface-700" aria-label={showPassword ? 'Hide password' : 'Show password'}>{showPassword ? <EyeOff size={20} strokeWidth={1.5} /> : <Eye size={20} strokeWidth={1.5} />}</button></div></div>
          {error && <div className="flex items-center gap-2 rounded-lg border border-danger-light bg-primary-50 px-3 py-2 text-sm text-danger-dark"><AlertCircle size={14} aria-hidden="true" /><span>{error}</span></div>}
          <button type="submit" disabled={loading} className="btn-primary h-[46px] w-full">{loading ? <Loader2 size={20} className="animate-spin" aria-hidden="true" /> : <LogIn size={20} strokeWidth={1.5} aria-hidden="true" />}Sign In</button>
        </form></div><div className="mt-5 text-center text-[13px] text-surface-600">Access is managed by your gateway administrator.</div>
      </div><p className="py-2 text-center font-operational text-[11px] text-surface-400">© 2026 llm_proxy · Connecting every thought to every model</p></section>
    </div></main>
    <button type="button" className="fixed bottom-4 right-4 inline-flex items-center gap-2 rounded-full border border-surface-200 bg-white px-3 py-2 text-sm text-surface-700 shadow-sm hover:bg-surface-50" aria-label="Contact support"><MessageCircle size={18} strokeWidth={1.5} /><span className="hidden sm:inline">Contact support</span></button>
    <p className="sr-only">Access to this gateway requires valid credentials</p>
  </main>;
}
