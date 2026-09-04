import { EmptyState } from '@/components/ui';
import { useLocale } from '@/i18n/context';

export function AuditLogPage() {
  const { t } = useLocale();
  return (
    <div className="space-y-6">
      <div className="border-b border-surface-200 pb-5"><h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{t.configAdmin.auditTitle}</h1><p className="mt-1 text-[14.5px] text-surface-600">{t.configAdmin.auditSubtitle}</p></div>
      <EmptyState title={t.adminUi.unavailable} description={t.configAdmin.auditHint} />
    </div>
  );
}
