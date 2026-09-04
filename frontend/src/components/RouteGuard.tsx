import { Navigate, Outlet } from 'react-router-dom';
import { LockKeyhole } from 'lucide-react';
import { useAuthStore } from '@/stores/authStore';

interface Props {
  requiredPermission?: string;
}

function ForbiddenPage() {
  return (
    <div className="flex items-center justify-center min-h-[60vh]">
      <div className="w-full max-w-md rounded-2xl border border-surface-200 bg-white p-8 text-center shadow-[0_8px_28px_-18px_rgba(10,10,10,0.18)]">
        <LockKeyhole className="mx-auto mb-4 text-surface-800" size={26} aria-hidden="true" />
        <h1 className="font-serif text-2xl font-bold text-surface-900">Access Denied</h1>
        <p className="mt-2 text-sm leading-6 text-surface-500">You don't have permission to access this page.</p>
        <a href="#/" className="btn-primary mt-6 inline-flex">
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
