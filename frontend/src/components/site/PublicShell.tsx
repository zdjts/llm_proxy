import { useEffect, useRef, useState } from 'react';
import { Menu, X } from 'lucide-react';
import { Link, NavLink, Outlet, useLocation } from 'react-router-dom';
import { useLocale } from '@/i18n/context';

const navigation = [{ to: '/', key: 'product' as const }, { to: '/models', key: 'models' as const }, { to: '/docs', key: 'documentation' as const }, { to: '/status', key: 'status' as const }];

export function PublicShell() {
  const { t } = useLocale();
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const wasOpen = useRef(false);
  const location = useLocation();
  useEffect(() => { setOpen(false); }, [location.pathname]);
  useEffect(() => { if (!open && wasOpen.current) buttonRef.current?.focus(); wasOpen.current = open; }, [open]);
  useEffect(() => { const close = (event: KeyboardEvent) => { if (event.key === 'Escape') setOpen(false); }; window.addEventListener('keydown', close); return () => window.removeEventListener('keydown', close); }, []);
  const nav = (mobile: boolean) => <nav className={mobile ? 'border-t border-surface-200 px-4 py-3 sm:hidden' : 'hidden items-center gap-5 sm:flex'} aria-label={mobile ? t.site.nav.publicMobileNavigation : t.site.nav.publicNavigation}>{navigation.map((item) => <NavLink key={item.to} to={item.to} className={mobile ? 'block py-2 text-sm text-surface-700' : ({ isActive }) => `text-sm ${isActive ? 'text-primary-700' : 'text-surface-600 hover:text-surface-900'}`}>{t.site.nav[item.key]}</NavLink>)}<Link className={mobile ? 'mt-2 inline-flex text-sm font-medium text-primary-700' : 'btn-primary'} to="/login">{t.site.nav.signIn}</Link></nav>;
  return <div className="min-h-screen bg-surface-50 text-surface-800"><header className="border-b border-surface-200 bg-white"><div className="mx-auto flex h-16 max-w-6xl items-center justify-between px-4 sm:px-6"><Link className="font-semibold text-surface-900" to="/">LLM Proxy</Link>{nav(false)}<button ref={buttonRef} type="button" className="rounded-md p-2 text-surface-700 sm:hidden" aria-label={open ? t.site.nav.closeNavigation : t.site.nav.openNavigation} aria-expanded={open} onClick={() => setOpen((value) => !value)}>{open ? <X size={20} /> : <Menu size={20} />}</button></div>{open && nav(true)}</header><main className="mx-auto min-h-[calc(100vh-8rem)] max-w-6xl px-4 py-10 sm:px-6"><Outlet /></main><footer className="border-t border-surface-200 bg-white"><div className="mx-auto flex max-w-6xl flex-col gap-2 px-4 py-5 text-xs text-surface-500 sm:flex-row sm:justify-between sm:px-6"><span>LLM Proxy</span><span>{t.site.nav.footerTagline}</span></div></footer></div>;
}
