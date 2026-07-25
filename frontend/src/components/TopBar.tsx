import { LogOut, User } from 'lucide-react';
import { useAuthStore } from '@/stores/authStore';
import { useNavigate } from 'react-router-dom';

export function TopBar() {
  const { user, logout, isAuthenticated } = useAuthStore();
  const navigate = useNavigate();

  if (!isAuthenticated || !user) return null;

  return (
    <div className="h-12 border-b border-surface-200 glass flex items-center justify-between px-6 sticky top-0 z-40">
      <div />
      <div className="flex items-center gap-3">
        <div className="flex items-center gap-2 text-sm text-surface-600">
          <div className="w-6 h-6 rounded-full bg-primary-100 flex items-center justify-center">
            <User size={12} className="text-primary-600" />
          </div>
          <span className="font-medium">{user.name}</span>
          <span className="text-surface-300 text-xs">({user.roles.join(', ')})</span>
        </div>
        <button
          onClick={() => { logout(); navigate('/login'); }}
          className="flex items-center gap-1 text-xs text-surface-400 hover:text-red-500 transition-colors"
        >
          <LogOut size={14} />
          <span>Logout</span>
        </button>
      </div>
    </div>
  );
}
