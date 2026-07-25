// v4.1 Track L: Aurora Glass design system — Design Token extension
// Replaces the inline glass/purple tokens already used in index.css with
// a systematic design language: Aurora gradient backgrounds, glassmorphism
// cards, glowing status indicators, skeleton loading, and number animations.

// New design tokens appended to existing index.css tokens

import React from 'react';

// ── Shared UI component stubs — to be fleshed out in T203-T209 ──

// Glass card wrapper
export function Card({ children, className = '', ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={`glass rounded-xl border border-white/20 p-6 backdrop-blur-xl bg-white/70 shadow-lg shadow-black/5 ${className}`} {...props}>
      {children}
    </div>
  );
}

// KPI stat card with count-up animation
export function StatCard({
  label, value, prefix = '', suffix = '', icon: Icon, trend,
}: {
  label: string; value: string | number; prefix?: string; suffix?: string;
  icon?: React.ComponentType<{ size?: number; className?: string }>; trend?: 'up' | 'down' | 'neutral';
}) {
  const trendColors = { up: 'text-green-500', down: 'text-red-500', neutral: 'text-surface-400' };
  return (
    <div className="glass rounded-xl border border-white/20 p-5 backdrop-blur-xl bg-white/70">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs font-medium text-surface-400 uppercase tracking-wider">{label}</span>
        {Icon && <Icon size={16} className="text-primary-400" />}
      </div>
      <div className="text-2xl font-bold text-surface-800 tabular-nums">
        {prefix}{typeof value === 'number' ? value.toLocaleString() : value}{suffix}
      </div>
      {trend && <div className={`text-xs mt-1 ${trendColors[trend]}`}>{trend === 'up' ? '↑' : trend === 'down' ? '↓' : '→'}</div>}
    </div>
  );
}

// Modal dialog
export function Modal({ open, onClose, title, children }: { open: boolean; onClose: () => void; title: string; children: React.ReactNode }) {
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 backdrop-blur-sm" onClick={onClose}>
      <div className="glass rounded-2xl border border-white/20 p-6 w-full max-w-md mx-4 shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold text-surface-800">{title}</h2>
          <button onClick={onClose} className="text-surface-400 hover:text-surface-600 text-xl leading-none">&times;</button>
        </div>
        {children}
      </div>
    </div>
  );
}

// Skeleton loading placeholder
export function Skeleton({ className = '' }: { className?: string }) {
  return <div className={`animate-pulse bg-surface-200 rounded ${className}`} />;
}

// Empty state with illustration
export function EmptyState({ title, description, action }: { title: string; description?: string; action?: React.ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center py-16 text-center">
      <div className="w-16 h-16 rounded-full bg-surface-100 flex items-center justify-center mb-4">
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="text-surface-300">
          <rect x="3" y="3" width="18" height="18" rx="2" />
          <path d="M12 8v4M12 16h.01" />
        </svg>
      </div>
      <h3 className="text-lg font-medium text-surface-600 mb-1">{title}</h3>
      {description && <p className="text-sm text-surface-400 max-w-sm">{description}</p>}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}

// Badge
export function Badge({ children, variant = 'default' }: { children: React.ReactNode; variant?: 'default' | 'success' | 'warning' | 'danger' | 'info' }) {
  const colors: Record<string, string> = {
    default: 'bg-surface-100 text-surface-600', success: 'bg-green-100 text-green-700',
    warning: 'bg-amber-100 text-amber-700', danger: 'bg-red-100 text-red-700', info: 'bg-blue-100 text-blue-700',
  };
  return <span className={`inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium ${colors[variant]}`}>{children}</span>;
}

// Status dot with glow
export function StatusDot({ status }: { status: 'healthy' | 'unhealthy' | 'probing' | 'unknown' }) {
  const colors: Record<string, string> = {
    healthy: 'bg-green-400 shadow-[0_0_8px_rgba(34,197,94,0.5)]',
    unhealthy: 'bg-red-400 shadow-[0_0_8px_rgba(239,68,68,0.5)]',
    probing: 'bg-amber-400 shadow-[0_0_8px_rgba(251,191,36,0.5)] animate-pulse',
    unknown: 'bg-gray-400',
  };
  return <span className={`inline-block w-2.5 h-2.5 rounded-full ${colors[status]}`} />;
}

// Table with glass styling
export function Table({ headers, rows, emptyMessage = 'No data' }: { headers: string[]; rows: (string | React.ReactNode)[][]; emptyMessage?: string }) {
  return (
    <div className="overflow-x-auto rounded-xl border border-surface-200">
      <table className="w-full text-sm">
        <thead>
          <tr className="bg-surface-50 border-b border-surface-200">
            {headers.map((h, i) => <th key={i} className="text-left px-4 py-3 font-medium text-surface-500 text-xs uppercase tracking-wider">{h}</th>)}
          </tr>
        </thead>
        <tbody className="divide-y divide-surface-100">
          {rows.length === 0 ? (
            <tr><td colSpan={headers.length} className="px-4 py-8 text-center text-surface-400">{emptyMessage}</td></tr>
          ) : rows.map((row, i) => (
            <tr key={i} className="hover:bg-surface-50/50 transition-colors">
              {row.map((cell, j) => <td key={j} className="px-4 py-3 text-surface-700">{cell}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// Button variants
export function Button({ children, variant = 'primary', size = 'md', disabled, loading, ...props }: React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: 'primary' | 'secondary' | 'danger' | 'ghost'; size?: 'sm' | 'md' | 'lg'; loading?: boolean }) {
  const bases: Record<string, string> = {
    primary: 'bg-primary-600 hover:bg-primary-700 text-white shadow-sm',
    secondary: 'bg-surface-100 hover:bg-surface-200 text-surface-700 border border-surface-200',
    danger: 'bg-red-600 hover:bg-red-700 text-white',
    ghost: 'hover:bg-surface-100 text-surface-600',
  };
  const sizes: Record<string, string> = { sm: 'px-3 py-1.5 text-xs', md: 'px-4 py-2 text-sm', lg: 'px-6 py-3 text-base' };
  return (
    <button
      className={`inline-flex items-center justify-center gap-2 rounded-lg font-medium transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed ${bases[variant]} ${sizes[size]}`}
      disabled={disabled || loading}
      {...props}
    >
      {loading && <span className="inline-block w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin" />}
      {children}
    </button>
  );
}
