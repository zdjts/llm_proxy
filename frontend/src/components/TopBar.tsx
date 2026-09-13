import type { RefObject } from 'react';
import { Menu } from 'lucide-react';
import { useLocale } from '@/i18n/context';

interface Props {
  menuButtonRef: RefObject<HTMLButtonElement | null>;
  onMenuClick: () => void;
}

export function TopBar({ menuButtonRef, onMenuClick }: Props) {
  const { t } = useLocale();

  return (
    <header className="sticky top-0 z-30 flex h-16 items-center justify-between border-b border-transparent bg-[#f7f6f2]/95 px-4 backdrop-blur sm:px-6 lg:px-10">
      <button ref={menuButtonRef} type="button" onClick={onMenuClick} className="rounded-md p-2 text-surface-600 hover:bg-surface-100 hover:text-surface-900 lg:hidden" aria-label={t.sidebar.openNavigation}>
        <Menu size={19} aria-hidden="true" />
      </button>
      <div className="hidden lg:block"><div className="font-serif text-lg font-bold text-surface-900">{t.overview.title}</div><div className="mt-0.5 text-[11px] text-surface-400">{t.sidebar.workspace}</div></div>
    </header>
  );
}
