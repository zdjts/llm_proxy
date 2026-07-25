import { Navigate, Outlet } from 'react-router-dom';
import { useAuthStore } from '@/stores/authStore';

interface Props {
  requiredPermission?: string;
}

function ForbiddenPage() {
  return (
    <div className="flex items-center justify-center min-h-[60vh]">
      <div className="text-center">
        <div className="text-6xl mb-4">🚫</div>
        <h1 className="text-2xl font-bold text-surface-800 mb-2">Access Denied</h1>
        <p className="text-surface-400 mb-6">You don't have permission to access this page.</p>
        <a href="#/" className="inline-flex items-center gap-2 px-4 py-2 bg-primary-600 text-white rounded-lg text-sm font-medium hover:bg-primary-700 transition-colors">
          Return to Dashboard
        </a>
      </div>
    </div>
  );
}

export function RouteGuard({ requiredPermission }: Props) {
  const { isAuthenticated, can } = useAuthStore();

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  if (requiredPermission && !can(requiredPermission)) {
    return <ForbiddenPage />;
  }

  return <Outlet />;
}
