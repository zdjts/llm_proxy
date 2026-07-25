import { NavLink } from 'react-router-dom';
import {
  LayoutDashboard, DollarSign, ListFilter, Key, BarChart3,
  Bell, HelpCircle, Radio, Users, Gauge, ChevronLeft, ChevronRight, Languages,
  Settings, Shield, Server, Wallet, Boxes, Activity, ShieldCheck,
} from 'lucide-react';
import { useLocale } from '@/i18n/context';
import { useAuthStore } from '@/stores/authStore';
import type { Locale, Messages } from '@/i18n/types';

interface Props { collapsed: boolean; onToggle: () => void; }

interface NavItem {
  to: string;
  label: string;
  icon: React.ComponentType<{ size?: number; className?: string }>;
  permission?: string;
  section?: string;
}

const allNavItems: NavItem[] = [
  // ── Overview ──
  { to: '/', label: 'overview', icon: LayoutDashboard, section: 'main' },
  { to: '/live', label: 'live monitor', icon: Activity, section: 'main' },

  // ── Configuration (Track M) ──
  { to: '/config/providers', label: 'providers', icon: Server, permission: 'providers.manage', section: 'config' },
  { to: '/config/pools', label: 'key pools', icon: Key, permission: 'keys.manage', section: 'config' },
  { to: '/config/routing', label: 'routing', icon: Boxes, permission: 'routing.edit', section: 'config' },
  { to: '/config/models', label: 'model catalog', icon: Boxes, permission: 'providers.manage', section: 'config' },

  // ── Budget & Users (Track N) ──
  { to: '/budgets', label: 'budgets', icon: Wallet, permission: 'billing.view', section: 'admin' },
  { to: '/admin/users', label: 'users', icon: Users, permission: 'team.manage', section: 'admin' },
  { to: '/admin/roles', label: 'roles', icon: Shield, permission: 'team.manage', section: 'admin' },
  { to: '/client-keys', label: 'client keys', icon: Users, section: 'admin' },

  // ── Monitoring (Track O) ──
  { to: '/cost', label: 'cost', icon: DollarSign, section: 'monitor' },
  { to: '/cost/drilldown', label: 'drilldown', icon: BarChart3, section: 'monitor' },
  { to: '/requests', label: 'requests', icon: ListFilter, section: 'monitor' },
  { to: '/keys', label: 'key health', icon: Key, section: 'monitor' },
  { to: '/traffic', label: 'traffic', icon: BarChart3, section: 'monitor' },
  { to: '/alerts', label: 'alerts', icon: Bell, section: 'monitor' },
  { to: '/quotas', label: 'quotas', icon: Gauge, section: 'monitor' },

  // ── Settings (Track P) ──
  { to: '/settings/audit', label: 'audit log', icon: ShieldCheck, permission: 'audit.view', section: 'settings' },
  { to: '/settings/system', label: 'system', icon: Settings, permission: 'providers.manage', section: 'settings' },
  { to: '/help', label: 'help', icon: HelpCircle, section: 'main' },
];

const sectionLabels: Record<string, keyof Messages['sidebar']['sections']> = {
  main: 'main',
  config: 'config',
  monitor: 'monitor',
  admin: 'admin',
  settings: 'settings',
};

const itemLabels: Record<string, Exclude<keyof Messages['sidebar'], 'sections'>> = {
  overview: 'overview',
  'live monitor': 'live',
  providers: 'providers',
  'key pools': 'keyPools',
  routing: 'routing',
  'model catalog': 'modelCatalog',
  budgets: 'budgets',
  users: 'users',
  roles: 'roles',
  'client keys': 'clientKeys',
  cost: 'cost',
  drilldown: 'drilldown',
  requests: 'requests',
  'key health': 'keys',
  traffic: 'traffic',
  alerts: 'alerts',
  quotas: 'quotas',
  'audit log': 'auditLog',
  system: 'system',
  help: 'help',
};

export function Sidebar({ collapsed, onToggle }: Props) {
  const { locale, t, setLocale } = useLocale();
  const { can } = useAuthStore();
  const nextLocale: Locale = locale === 'en' ? 'zh-CN' : 'en';

  const visibleItems = allNavItems.filter((item) => !item.permission || can(item.permission));

  // Group by section
  const sections = new Map<string, NavItem[]>();
  for (const item of visibleItems) {
    const s = item.section || 'main';
    if (!sections.has(s)) sections.set(s, []);
    sections.get(s)!.push(item);
  }

  return (
    <aside className={`fixed top-0 left-0 h-screen z-50 flex flex-col transition-all duration-300 ${collapsed ? 'w-16' : 'w-56'}`}>
      <div className="flex-1 glass flex flex-col border-r border-surface-200">
        <div className={`flex items-center gap-3 px-4 h-16 border-b border-surface-200 ${collapsed ? 'justify-center' : ''}`}>
          <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-primary-500 to-primary-700 flex items-center justify-center text-white font-bold text-sm flex-shrink-0 shadow-md shadow-primary-500/20">P</div>
          {!collapsed && <div><div className="text-sm font-semibold text-surface-800">{t.sidebar.brand}</div><div className="text-[10px] text-primary-500 uppercase tracking-wider font-medium">{t.sidebar.version}</div></div>}
        </div>

        <nav className="flex-1 py-3 px-2 space-y-4 overflow-y-auto">
          {Array.from(sections.entries()).map(([section, items]) => (
            <div key={section}>
              {!collapsed && (
                <div className="px-3 py-1 text-[10px] font-semibold uppercase tracking-widest text-surface-300">
                  {t.sidebar.sections[sectionLabels[section]]}
                </div>
              )}
              <div className="space-y-0.5">
                {items.map((item) => (
                  <NavLink
                    key={item.to}
                    to={item.to}
                    end={item.to === '/'}
                    className={({ isActive }) =>
                      `flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all duration-200 group ${
                        isActive ? 'bg-primary-50 text-primary-700 border border-primary-200 shadow-sm'
                        : 'text-surface-500 hover:text-surface-700 hover:bg-surface-100'
                      } ${collapsed ? 'justify-center px-2' : ''}`
                    }
                    title={collapsed ? t.sidebar[itemLabels[item.label]] : undefined}
                  >
                    <item.icon size={18} className="flex-shrink-0" />
                    {!collapsed && <span className="capitalize">{t.sidebar[itemLabels[item.label]]}</span>}
                  </NavLink>
                ))}
              </div>
            </div>
          ))}
        </nav>

        <button onClick={() => setLocale(nextLocale)} className="h-10 border-t border-surface-200 flex items-center justify-center gap-1.5 text-surface-500 hover:text-primary-600 transition-colors text-xs font-medium" title={t.common.switchLang}>
          <Languages size={14} />
          {!collapsed && <span>{nextLocale === 'zh-CN' ? '中文' : 'EN'}</span>}
        </button>

        <button onClick={onToggle} className="h-10 border-t border-surface-200 flex items-center justify-center text-surface-400 hover:text-surface-600 transition-colors">
          {collapsed ? <ChevronRight size={16} /> : <ChevronLeft size={16} />}
        </button>
      </div>
    </aside>
  );
}
