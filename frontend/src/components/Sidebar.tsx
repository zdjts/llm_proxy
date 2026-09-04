import { useEffect, useRef } from 'react';
import { NavLink } from 'react-router-dom';
import { Activity, BarChart3, Bell, Boxes, DollarSign, Gauge, HelpCircle, Key, Languages, LayoutDashboard, Server, Settings, Shield, ShieldCheck, Users, Wallet, X } from 'lucide-react';
import { useLocale } from '@/i18n/context';
import { useAuthStore } from '@/stores/authStore';
import type { Locale, Messages } from '@/i18n/types';

interface Props { mobileOpen: boolean; onMobileClose: () => void; }
interface NavItem { to: string; label: string; icon: React.ComponentType<{ size?: number; className?: string }>; permission?: string; section?: string; }

const allNavItems: NavItem[] = [
  { to: '/console', label: 'overview', icon: LayoutDashboard, section: 'main' }, { to: '/live', label: 'live monitor', icon: Activity, section: 'main' },
  { to: '/config', label: 'config console', icon: Settings, permission: 'audit.view', section: 'config' }, { to: '/config/providers', label: 'providers', icon: Server, permission: 'providers.manage', section: 'config' }, { to: '/config/pools', label: 'key pools', icon: Key, permission: 'keys.manage', section: 'config' }, { to: '/config/routing', label: 'routing', icon: Boxes, permission: 'routing.edit', section: 'config' }, { to: '/config/models', label: 'model catalog', icon: Boxes, permission: 'providers.manage', section: 'config' },
  { to: '/budgets', label: 'budgets', icon: Wallet, permission: 'billing.view', section: 'admin' }, { to: '/admin/users', label: 'users', icon: Users, permission: 'team.manage', section: 'admin' }, { to: '/admin/roles', label: 'roles', icon: Shield, permission: 'team.manage', section: 'admin' }, { to: '/client-keys', label: 'client keys', icon: Users, section: 'admin' },
  { to: '/cost', label: 'cost', icon: DollarSign, section: 'monitor' }, { to: '/cost/drilldown', label: 'drilldown', icon: BarChart3, section: 'monitor' }, { to: '/requests', label: 'requests', icon: Activity, section: 'monitor' }, { to: '/keys', label: 'key health', icon: Key, section: 'monitor' }, { to: '/traffic', label: 'traffic', icon: BarChart3, section: 'monitor' }, { to: '/alerts', label: 'alerts', icon: Bell, section: 'monitor' }, { to: '/quotas', label: 'quotas', icon: Gauge, section: 'monitor' },
  { to: '/settings/audit', label: 'audit log', icon: ShieldCheck, permission: 'audit.view', section: 'settings' }, { to: '/settings/system', label: 'system', icon: Settings, permission: 'providers.manage', section: 'settings' }, { to: '/help', label: 'help', icon: HelpCircle, section: 'main' },
];
const sectionLabels: Record<string, keyof Messages['sidebar']['sections']> = { main: 'main', config: 'config', monitor: 'monitor', admin: 'admin', settings: 'settings' };
const itemLabels: Record<string, Exclude<keyof Messages['sidebar'], 'sections'>> = { overview: 'overview', 'live monitor': 'live', providers: 'providers', 'key pools': 'keyPools', routing: 'routing', 'model catalog': 'modelCatalog', budgets: 'budgets', users: 'users', roles: 'roles', 'client keys': 'clientKeys', cost: 'cost', drilldown: 'drilldown', requests: 'requests', 'key health': 'keys', traffic: 'traffic', alerts: 'alerts', quotas: 'quotas', 'audit log': 'auditLog', system: 'system', help: 'help', 'config console': 'configConsole' };
const focusableSelector = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function Sidebar({ mobileOpen, onMobileClose }: Props) {
  const { locale, t, setLocale } = useLocale();
  const { can } = useAuthStore();
  const drawerRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const visibleItems = allNavItems.filter((item) => !item.permission || can(item.permission));
  const sections = new Map<string, NavItem[]>();
  const nextLocale: Locale = locale === 'en' ? 'zh-CN' : 'en';

  for (const item of visibleItems) {
    const section = item.section || 'main';
    sections.set(section, [...(sections.get(section) || []), item]);
  }

  useEffect(() => {
    if (!mobileOpen) return;
    closeRef.current?.focus();
    const drawer = drawerRef.current;
    if (!drawer) return;
    const trapFocus = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return;
      const focusable = Array.from(drawer.querySelectorAll<HTMLElement>(focusableSelector));
      if (focusable.length === 0) { event.preventDefault(); return; }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    drawer.addEventListener('keydown', trapFocus);
    return () => drawer.removeEventListener('keydown', trapFocus);
  }, [mobileOpen]);

  const navigation = (isMobile: boolean) => <div className="flex h-full flex-col bg-white">
    <div className="flex h-7 items-center gap-2 border-b border-surface-200 px-4 font-operational text-[10px] uppercase tracking-[0.18em] text-surface-500"><span className="h-1.5 w-1.5 rounded-full bg-surface-900" />{t.sidebar.workspace}</div>
    <div className="flex min-h-[72px] items-center border-b border-surface-200 px-4">
      <div className="flex h-9 w-9 items-center justify-center rounded-lg border border-surface-200 font-serif text-lg font-bold text-surface-900">P</div>
      <div className="ml-3 min-w-0"><div className="truncate font-serif text-lg font-bold tracking-wide text-surface-900">{t.sidebar.brand}</div><div className="mt-0.5 font-operational text-[10px] uppercase tracking-[0.15em] text-surface-400">{t.sidebar.version}</div></div>
      {isMobile && <button ref={closeRef} type="button" onClick={onMobileClose} className="ml-auto rounded-md p-2 text-surface-500 hover:bg-surface-100 hover:text-surface-900" aria-label={t.sidebar.closeNavigation}><X size={18} aria-hidden="true" /></button>}
    </div>
    <nav className="min-h-0 flex-1 overflow-y-auto py-2" aria-label={t.sidebar.navigation}>{Array.from(sections.entries()).map(([section, items]) => <div key={section} className="border-b border-surface-100 py-2 last:border-b-0"><div className="px-4 pb-1.5 pt-1 font-operational text-[10px] font-medium uppercase tracking-[0.22em] text-surface-400">{t.sidebar.sections[sectionLabels[section]]}</div><div>{items.map((item) => <NavLink key={item.to} to={item.to} end={item.to === '/console'} onClick={isMobile ? onMobileClose : undefined} className={({ isActive }) => `relative flex items-center gap-3 px-5 py-2 text-[13px] tracking-wide transition-colors ${isActive ? 'bg-surface-50 font-medium text-surface-900 before:absolute before:bottom-2 before:left-1 before:top-2 before:w-0.5 before:rounded before:bg-surface-900' : 'text-surface-600 hover:bg-surface-50 hover:text-surface-900'}`}><item.icon size={16} className="shrink-0" aria-hidden="true" /><span>{t.sidebar[itemLabels[item.label]]}</span></NavLink>)}</div></div>)}</nav>
    <button type="button" onClick={() => setLocale(nextLocale)} className="flex h-11 items-center gap-2 border-t border-surface-200 px-5 text-xs font-medium text-surface-600 transition-colors hover:bg-surface-100 hover:text-surface-900" title={t.common.switchLang}><Languages size={15} aria-hidden="true" /><span>{nextLocale === 'zh-CN' ? '中文' : 'EN'}</span></button>
  </div>;

  return <>
    <aside className="fixed inset-y-0 left-0 z-40 hidden w-64 border-r border-surface-200 bg-white lg:block">{navigation(false)}</aside>
    {mobileOpen && <><button type="button" className="fixed inset-0 z-40 bg-surface-900/20 lg:hidden" onClick={onMobileClose} aria-label={t.sidebar.closeNavigation} /><aside ref={drawerRef} className="fixed inset-y-0 left-0 z-50 w-72 border-r border-surface-200 shadow-xl lg:hidden" aria-modal="true" role="dialog" aria-label={t.sidebar.navigation}>{navigation(true)}</aside></>}
  </>;
}
