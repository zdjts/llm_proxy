import { Badge, Card } from '@/components/ui';
import { useLocale } from '@/i18n/context';

const BUILT_IN_ROLES = [
  { id: 'owner', name: 'Owner', permissions: ['* (all)'] },
  { id: 'admin', name: 'Admin', permissions: ['providers.manage', 'keys.manage', 'routing.edit', 'pipeline.edit', 'quotas.manage', 'billing.view', 'alerts.manage', 'audit.view'] },
  { id: 'operator', name: 'Operator', permissions: ['keys.manage', 'routing.simulate', 'pipeline.view', 'alerts.view'] },
  { id: 'billing', name: 'Billing', permissions: ['billing.view', 'billing.export', 'quotas.manage', 'alerts.view'] },
  { id: 'readonly', name: 'Read Only', permissions: ['routing.simulate', 'pipeline.view', 'billing.view', 'alerts.view', 'audit.view'] },
  { id: 'portal_user', name: 'Portal User', permissions: ['portal.self'] },
];

export function RoleListPage() {
  const { t } = useLocale();
  return <div className="space-y-6">
    <header className="border-b border-surface-200 pb-5"><h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.configAdmin.rolesTitle}</h1><p className="mt-1 text-[14.5px] text-surface-600">{t.configAdmin.rolesSubtitle}</p></header>
    <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
      {BUILT_IN_ROLES.map((role) => <Card key={role.id} className="p-5"><h2 className="mb-3 font-semibold text-surface-900">{role.name}</h2><div className="flex flex-wrap gap-1.5">{role.permissions.map((permission) => <Badge key={permission} variant="info">{permission}</Badge>)}</div></Card>)}
    </div>
  </div>;
}
