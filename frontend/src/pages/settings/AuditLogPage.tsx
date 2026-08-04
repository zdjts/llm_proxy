import { EmptyState } from '@/components/ui';
import { useLocale } from '@/i18n/context';

export function AuditLogPage() {
  const { t } = useLocale();
  return (
    <div className="space-y-6">
      <div className="border-b border-surface-200 pb-5"><h1 className="text-xl font-semibold text-surface-900 sm:text-2xl">{t.configAdmin.auditTitle}</h1><p className="mt-1 text-sm text-surface-500">{t.configAdmin.auditSubtitle}</p></div>
      <EmptyState title={t.adminUi.unavailable} description={t.configAdmin.auditHint} />
    </div>
  );
}
