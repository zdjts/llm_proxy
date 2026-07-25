import { Card, EmptyState } from '@/components/ui';

export function AuditLogPage() {
  return (
    <div className="space-y-6">
      <div><h1 className="text-2xl font-bold text-surface-800">Audit Log</h1><p className="text-sm text-surface-400 mt-1">Track all configuration changes and sensitive operations</p></div>
      <EmptyState title="Audit log available via API" description="Use GET /admin/api/audit-trail?event_type=provider.create to query audit events. Full UI with diff visualization coming in next iteration." />
    </div>
  );
}
