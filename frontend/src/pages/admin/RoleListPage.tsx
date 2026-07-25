import { Card, Badge } from '@/components/ui';

const BUILT_IN_ROLES = [
  { id: 'owner', name: 'Owner', permissions: ['* (all)'] },
  { id: 'admin', name: 'Admin', permissions: ['providers.manage', 'keys.manage', 'routing.edit', 'pipeline.edit', 'quotas.manage', 'billing.view', 'alerts.manage', 'audit.view'] },
  { id: 'operator', name: 'Operator', permissions: ['keys.manage', 'routing.simulate', 'pipeline.view', 'alerts.view'] },
  { id: 'billing', name: 'Billing', permissions: ['billing.view', 'billing.export', 'quotas.manage', 'alerts.view'] },
  { id: 'readonly', name: 'Read Only', permissions: ['routing.simulate', 'pipeline.view', 'billing.view', 'alerts.view', 'audit.view'] },
  { id: 'portal_user', name: 'Portal User', permissions: ['portal.self'] },
];

export function RoleListPage() {
  return (
    <div className="space-y-6">
      <div><h1 className="text-2xl font-bold text-surface-800">Roles</h1><p className="text-sm text-surface-400 mt-1">Built-in RBAC roles and their permissions</p></div>
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
        {BUILT_IN_ROLES.map(role => (
          <Card key={role.id} className="p-5">
            <h3 className="font-semibold text-surface-800 capitalize mb-2">{role.name}</h3>
            <div className="flex flex-wrap gap-1">
              {role.permissions.map(p => <Badge key={p} variant="info">{p}</Badge>)}
            </div>
          </Card>
        ))}
      </div>
    </div>
  );
}
