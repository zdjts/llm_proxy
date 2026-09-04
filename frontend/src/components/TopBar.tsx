import type { RefObject } from 'react';
import { LogOut, Menu, User } from 'lucide-react';
import { useAuthStore } from '@/stores/authStore';
import { useNavigate } from 'react-router-dom';
import { useLocale } from '@/i18n/context';

interface Props {
  menuButtonRef: RefObject<HTMLButtonElement | null>;
  onMenuClick: () => void;
}

export function TopBar({ menuButtonRef, onMenuClick }: Props) {
  const { user, logout, isAuthenticated } = useAuthStore();
  const { t } = useLocale();
  const navigate = useNavigate();

  if (!isAuthenticated || !user) return null;

  return (
    <header className="sticky top-0 z-30 flex h-16 items-center justify-between border-b border-transparent bg-[#f7f6f2]/95 px-4 backdrop-blur sm:px-6 lg:px-10">
      <button ref={menuButtonRef} type="button" onClick={onMenuClick} className="rounded-md p-2 text-surface-600 hover:bg-surface-100 hover:text-surface-900 lg:hidden" aria-label={t.sidebar.openNavigation}>
        <Menu size={19} aria-hidden="true" />
      </button>
      <div className="hidden lg:block"><div className="font-serif text-lg font-bold text-surface-900">{t.overview.title}</div><div className="mt-0.5 text-[11px] text-surface-400">{t.sidebar.workspace}</div></div>
      <div className="ml-auto flex min-w-0 items-center gap-3">
        <div className="flex min-w-0 items-center gap-2 text-sm text-surface-600">
          <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-surface-200 bg-white text-surface-600"><User size={14} aria-hidden="true" /></div>
          <span className="max-w-32 truncate font-medium text-surface-800 sm:max-w-48">{user.name}</span>
          <span className="hidden max-w-48 truncate font-operational text-[10px] text-surface-400 md:inline">{user.roles.join(', ')}</span>
        </div>
        <button type="button" onClick={() => { logout(); navigate('/login'); }} className="inline-flex items-center gap-1.5 rounded-full px-2 py-1.5 text-xs font-medium text-surface-500 hover:bg-surface-100 hover:text-surface-900" aria-label={t.sidebar.logout}>
          <LogOut size={15} aria-hidden="true" />
          <span className="hidden sm:inline">{t.sidebar.logout}</span>
        </button>
      </div>
    </header>
  );
}
