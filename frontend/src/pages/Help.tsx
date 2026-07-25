import { useQuery } from '@tanstack/react-query';
import axios from 'axios';
import { BookOpen } from 'lucide-react';
import { useLocale } from '@/i18n/context';

export function HelpPage() {
  const { t } = useLocale();
  const { data } = useQuery({
    queryKey: ['help'],
    queryFn: async () => { const { data } = await axios.get<{ runbook: string }>('/admin/help?format=json'); return data; },
  });

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-gradient">{t.help.title}</h1>
        <p className="text-sm text-surface-500 mt-1">{t.help.subtitle}</p>
      </div>
      <div className="glass-card p-6">
        {data?.runbook ? (
          <div className="prose max-w-none"><pre className="text-sm text-surface-600 whitespace-pre-wrap font-mono leading-relaxed bg-transparent p-0">{data.runbook}</pre></div>
        ) : (
          <div className="flex items-center justify-center py-16 text-surface-400">
            <div className="text-center"><BookOpen size={32} className="mx-auto mb-3 text-surface-300" /><p>{t.help.loading}</p></div>
          </div>
        )}
      </div>
    </div>
  );
}
