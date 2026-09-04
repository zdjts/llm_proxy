import { useEffect, useRef, useState } from 'react';
import { Link, NavLink, Outlet, useLocation } from 'react-router-dom';
import { LogOut, Menu, User, X } from 'lucide-react';
import { accountRoutes, type SiteGroup } from '@/routes/manifest';
import { useLocale } from '@/i18n/context';
import { useAuthStore } from '@/stores/authStore';

const groups: SiteGroup[] = ['workspace', 'developer', 'commercial', 'community', 'activities', 'operator'];

export function AccountShell() {
  const { t } = useLocale();
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const wasOpen = useRef(false);
  const location = useLocation();
  const { user, logout } = useAuthStore();

  useEffect(() => { setOpen(false); }, [location.pathname]);
  useEffect(() => {
    if (!open && wasOpen.current) buttonRef.current?.focus();
    wasOpen.current = open;
  }, [open]);
  useEffect(() => {
    const close = (event: KeyboardEvent) => { if (event.key === 'Escape') setOpen(false); };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, []);

  const links = (label: string) => (
    <nav className="min-h-0 flex-1 overflow-y-auto py-2" aria-label={label}>
      {groups.map((group) => {
        const routes = accountRoutes.filter((route) => route.group === group);
        return (
          <section key={group} className="border-b border-surface-100 py-2 last:border-b-0">
            <p className="px-4 pb-1.5 pt-1 font-operational text-[10px] font-medium uppercase tracking-[0.22em] text-surface-400">
              {t.site.account.groups[group]}
            </p>
            <div>
              {routes.map((route) => {
                const Icon = route.icon;
                return (
                  <NavLink
                    key={route.path}
                    to={route.path}
                    className={({ isActive }) =>
                      `relative flex items-center gap-3 px-5 py-2 text-[13px] tracking-wide transition-colors ${
                        isActive
                          ? 'bg-surface-50 font-medium text-surface-900 before:absolute before:bottom-2 before:left-1 before:top-2 before:w-0.5 before:rounded before:bg-surface-900'
                          : 'text-surface-600 hover:bg-surface-50 hover:text-surface-900'
                      }`
                    }
                  >
                    <Icon size={16} className="shrink-0" aria-hidden="true" />
                    <span className="truncate">{t.site.routes[route.key].title}</span>
                  </NavLink>
                );
              })}
            </div>
          </section>
        );
      })}
    </nav>
  );

  const navigation = (label: string, mobile = false) => (
    <div className="flex h-full flex-col bg-white">
      <div className="flex h-7 items-center gap-2 border-b border-surface-200 px-4 font-operational text-[10px] uppercase tracking-[0.18em] text-surface-500">
        <span className="h-1.5 w-1.5 rounded-full bg-surface-900" />
        {t.sidebar.workspace}
      </div>
      <div className="flex min-h-[72px] items-center border-b border-surface-200 px-4">
        <Link className="flex min-w-0 items-center" to="/">
          <span className="brand-mark-fallback">P</span>
          <span className="ml-3 min-w-0">
            <span className="block truncate font-serif text-lg font-bold tracking-wide text-surface-900">llm_proxy</span>
            <span className="mt-0.5 block font-operational text-[10px] uppercase tracking-[0.15em] text-surface-400">{t.sidebar.version}</span>
          </span>
        </Link>
        {mobile && (
          <button
            type="button"
            onClick={() => setOpen(false)}
            className="ml-auto rounded-md p-2 text-surface-500 hover:bg-surface-100 hover:text-surface-900"
            aria-label={t.site.nav.closeNavigation}
          >
            <X size={18} aria-hidden="true" />
          </button>
        )}
      </div>
      {links(label)}
      <div className="flex items-center gap-2 border-t border-surface-200 px-4 py-3 text-xs text-surface-600">
        <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-surface-200 bg-surface-50">
          <User size={14} aria-hidden="true" />
        </span>
        <span className="min-w-0 flex-1 truncate font-medium text-surface-800">{user?.name}</span>
        <button type="button" onClick={logout} className="rounded-md p-1.5 text-surface-500 hover:bg-surface-100 hover:text-surface-900" aria-label={t.sidebar.logout}>
          <LogOut size={15} aria-hidden="true" />
        </button>
      </div>
    </div>
  );

  return (
    <div className="user-console-page min-h-screen text-surface-800">
      <header className="flex h-16 items-center justify-between border-b border-surface-200 bg-[#f7f6f2] px-4 lg:hidden">
        <Link className="font-serif text-lg font-bold text-surface-900" to="/">llm_proxy</Link>
        <button
          ref={buttonRef}
          type="button"
          className="rounded-md p-2 text-surface-600 hover:bg-surface-100 hover:text-surface-900"
          aria-label={open ? t.site.nav.closeNavigation : t.site.nav.openNavigation}
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          {open ? <X size={20} /> : <Menu size={20} />}
        </button>
      </header>
      {open && <aside className="fixed inset-x-0 bottom-0 top-16 z-50 border-r border-surface-200 bg-white lg:hidden">{navigation(t.site.nav.accountMobileNavigation, true)}</aside>}
      <aside className="deck-sidebar-local">{navigation(t.site.nav.accountNavigation)}</aside>
      <main className="deck-main px-4 py-6 sm:px-6 lg:px-10 lg:py-8">
        <div className="mx-auto max-w-[1480px]">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
