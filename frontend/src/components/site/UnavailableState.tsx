import { LockKeyhole } from 'lucide-react';
import { Button, Card } from '@/components/ui';
import { useLocale } from '@/i18n/context';

export function UnavailableState({ title, description, action }: { title: string; description: string; action?: string }) {
  const { t } = useLocale();
  return <Card className="mx-auto max-w-2xl py-12 text-center">
    <LockKeyhole className="mx-auto mb-4 text-surface-400" size={28} aria-hidden="true" />
    <h1 className="font-serif text-2xl font-bold text-surface-900">{title}</h1>
    <p className="mx-auto mt-2 max-w-lg text-sm text-surface-500">{description}</p>
    <Button className="mt-6" disabled aria-disabled="true">{action || t.site.actions.unavailable}</Button>
  </Card>;
}
