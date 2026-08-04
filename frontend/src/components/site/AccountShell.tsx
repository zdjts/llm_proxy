import { useEffect, useRef, useState } from 'react';
import { Link, NavLink, Outlet, useLocation } from 'react-router-dom';
import { Menu, X } from 'lucide-react';
import { accountRoutes, type SiteGroup } from '@/routes/manifest';
import { useLocale } from '@/i18n/context';

const groups: SiteGroup[] = ['workspace', 'developer', 'commercial', 'community', 'activities', 'operator'];

export function AccountShell() {
  const { t } = useLocale();
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const wasOpen = useRef(false);
  const location = useLocation();
  useEffect(() => { setOpen(false); }, [location.pathname]);
  useEffect(() => { if (!open && wasOpen.current) buttonRef.current?.focus(); wasOpen.current = open; }, [open]);
  useEffect(() => { const close = (event: KeyboardEvent) => { if (event.key === 'Escape') setOpen(false); }; window.addEventListener('keydown', close); return () => window.removeEventListener('keydown', close); }, []);
  const links = (label: string) => <nav className="space-y-5" aria-label={label}>{groups.map((group) => { const routes = accountRoutes.filter((route) => route.group === group); return <section key={group}><p className="mb-1 px-3 text-xs font-medium uppercase text-surface-400">{t.site.account.groups[group]}</p><div className="space-y-1">{routes.map((route) => { const Icon = route.icon; return <NavLink key={route.path} to={route.path} className={({ isActive }) => `flex items-center gap-2 rounded-md px-3 py-2 text-sm ${isActive ? 'bg-surface-100 text-surface-900' : 'text-surface-600 hover:bg-surface-50 hover:text-surface-900'}`}><Icon size={16} aria-hidden="true" />{t.site.routes[route.key].title}</NavLink>; })}</div></section>; })}</nav>;
  return <div className="min-h-screen bg-surface-50 text-surface-800"><header className="flex h-14 items-center justify-between border-b border-surface-200 bg-white px-4 lg:hidden"><Link className="font-semibold" to="/">LLM Proxy</Link><button ref={buttonRef} type="button" className="rounded-md p-2" aria-label={open ? t.site.nav.closeNavigation : t.site.nav.openNavigation} aria-expanded={open} onClick={() => setOpen((value) => !value)}>{open ? <X size={20} /> : <Menu size={20} />}</button></header>{open && <aside className="max-h-[calc(100vh-3.5rem)] overflow-y-auto border-b border-surface-200 bg-white p-4 lg:hidden">{links(t.site.nav.accountMobileNavigation)}</aside>}<aside className="fixed inset-y-0 hidden w-60 overflow-y-auto border-r border-surface-200 bg-white p-4 lg:block"><Link className="mb-7 block font-semibold text-surface-900" to="/">LLM Proxy</Link>{links(t.site.nav.accountNavigation)}</aside><main className="min-h-screen px-4 py-6 sm:px-6 lg:ml-60 lg:px-8"><div className="mx-auto max-w-6xl"><Outlet /></div></main></div>;
}
