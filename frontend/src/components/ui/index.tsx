import { AlertCircle, CircleAlert, LoaderCircle } from 'lucide-react';
import type React from 'react';
import { useLocale } from '@/i18n/context';

function join(...classes: Array<string | undefined>) {
  return classes.filter(Boolean).join(' ');
}

export function PageHeader({ title, description, actions, className }: {
  title: string;
  description?: string;
  actions?: React.ReactNode;
  className?: string;
}) {
  return <div className={join('flex flex-wrap items-end justify-between gap-4 border-b border-surface-200 pb-5', className)}><div><h1 className="font-serif text-[26px] font-bold tracking-[0.005em] text-surface-900 sm:text-[32px]">{title}</h1>{description && <p className="mt-1 text-[14.5px] text-surface-600">{description}</p>}</div>{actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}</div>;
}

export function Card({ children, className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={join('rounded-2xl border border-surface-200 bg-white p-[18px] shadow-[0_8px_28px_-18px_rgba(10,10,10,0.18)] transition-all duration-300 hover:-translate-y-px hover:shadow-[0_14px_34px_-20px_rgba(10,10,10,0.24)]', className)} {...props}>{children}</div>;
}

export function StatCard({ label, value, prefix = '', suffix = '', icon: Icon, trend, className }: {
  label: string; value: string | number; prefix?: string; suffix?: string;
  icon?: React.ComponentType<{ size?: number; className?: string }>;
  trend?: 'up' | 'down' | 'neutral'; className?: string;
}) {
  const { t } = useLocale();
  const trendText = trend === 'up' ? 'text-success-dark' : trend === 'down' ? 'text-danger-dark' : 'text-surface-400';
  const trendLabel = trend === 'up' ? t.common.uiIncreasing : trend === 'down' ? t.common.uiDecreasing : t.common.uiUnchanged;
  return <Card className={join('p-4', className)}><div className="flex items-start gap-3"><div className="min-w-0 flex-1"><span className="mb-1.5 block text-xs font-medium text-surface-500">{label}</span><div className="font-operational text-2xl font-bold tracking-tight text-surface-900">{prefix}{typeof value === 'number' ? value.toLocaleString() : value}{suffix}</div>{trend && <div className={join('mt-1 text-xs', trendText)}>{trendLabel}</div>}</div>{Icon && <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border border-surface-200 bg-surface-50 text-surface-800"><Icon size={18} aria-hidden="true" /></span>}</div></Card>;
}

export function Modal({ open, onClose, title, children }: { open: boolean; onClose: () => void; title: string; children: React.ReactNode }) {
  const { t } = useLocale();
  if (!open) return null;
  return <div className="fixed inset-0 z-50 flex items-center justify-center bg-surface-900/30 p-4 backdrop-blur-sm" role="presentation" onMouseDown={onClose}><div className="w-full max-w-md rounded-2xl border border-surface-200 bg-white p-6 shadow-[0_24px_64px_-28px_rgba(0,0,0,0.38)]" role="dialog" aria-modal="true" aria-label={title} onMouseDown={(event) => event.stopPropagation()}><div className="mb-5 flex items-center justify-between gap-4 border-b border-surface-200 pb-4"><h2 className="font-serif text-xl font-bold text-surface-900">{title}</h2><button type="button" onClick={onClose} className="rounded-lg px-2 py-1 text-sm text-surface-500 hover:bg-surface-100 hover:text-surface-800" aria-label={t.common.uiClose}>{t.common.uiClose}</button></div>{children}</div></div>;
}

export function Skeleton({ className }: { className?: string }) {
  const { t } = useLocale();
  return <div className={join('animate-pulse rounded-md bg-surface-100', className)} aria-label={t.common.uiLoading} />;
}

export function EmptyState({ title, description, action, className }: { title: string; description?: string; action?: React.ReactNode; className?: string }) {
  return <div className={join('flex flex-col items-center justify-center border border-dashed border-surface-200 px-6 py-14 text-center', className)}><CircleAlert size={24} className="mb-3 text-surface-400" aria-hidden="true" /><h3 className="text-sm font-semibold text-surface-700">{title}</h3>{description && <p className="mt-1 max-w-sm text-sm text-surface-500">{description}</p>}{action && <div className="mt-4">{action}</div>}</div>;
}

export function ErrorState({ title, description, action, className }: { title?: string; description?: string; action?: React.ReactNode; className?: string }) {
  const { t } = useLocale();
  return <div className={join('flex flex-col items-center justify-center border border-danger-light bg-primary-50 px-6 py-10 text-center', className)}><AlertCircle size={22} className="mb-3 text-danger" aria-hidden="true" /><h3 className="text-sm font-semibold text-danger-dark">{title || t.common.uiUnableToLoad}</h3>{description && <p className="mt-1 max-w-sm text-sm text-surface-600">{description}</p>}{action && <div className="mt-4">{action}</div>}</div>;
}

export function Badge({ children, variant = 'default', className }: { children: React.ReactNode; variant?: 'default' | 'success' | 'warning' | 'danger' | 'info'; className?: string }) {
  const colors = { default: 'bg-surface-100 text-surface-600', success: 'bg-success-light text-success-dark', warning: 'bg-warning-light text-warning-dark', danger: 'bg-danger-light text-danger-dark', info: 'bg-info-light text-info-dark' };
  return <span className={join('inline-flex items-center gap-1 rounded-md border border-transparent px-2 py-0.5 text-xs font-medium', colors[variant], className)}>{children}</span>;
}

export function StatusDot({ status }: { status: 'healthy' | 'unhealthy' | 'probing' | 'unknown' }) {
  const colors = { healthy: 'bg-success', unhealthy: 'bg-danger', probing: 'bg-warning animate-pulse', unknown: 'bg-surface-400' };
  return <span className={join('inline-block h-2 w-2 rounded-full', colors[status])} aria-label={status} />;
}

export function Table({ headers, rows, emptyMessage }: { headers: string[]; rows: React.ReactNode[][]; emptyMessage?: string }) {
  const { t } = useLocale();
  return <div className="overflow-x-auto rounded-2xl border border-surface-200 bg-white p-1"><table className="min-w-full text-sm"><thead><tr>{headers.map((header) => <th key={header} scope="col" className="whitespace-nowrap border-b border-surface-200 px-5 py-4 text-left font-operational text-[10px] font-semibold uppercase tracking-[0.1em] text-surface-400">{header}</th>)}</tr></thead><tbody>{rows.length === 0 ? <tr><td colSpan={headers.length} className="px-4 py-10 text-center text-sm text-surface-500">{emptyMessage || t.common.uiNoData}</td></tr> : rows.map((row, index) => <tr key={index} className="border-b border-surface-100 last:border-0 hover:bg-surface-50">{row.map((cell, cellIndex) => <td key={cellIndex} className="whitespace-nowrap px-5 py-4 text-surface-700">{cell}</td>)}</tr>)}</tbody></table></div>;
}

export function Pagination({ page, pageCount, onPageChange, className }: { page: number; pageCount: number; onPageChange: (page: number) => void; className?: string }) {
  const { t } = useLocale();
  if (pageCount <= 1) return null;
  return <nav className={join('flex items-center justify-between gap-3', className)} aria-label={t.common.uiPagination}><span className="font-operational text-xs text-surface-500">{page} / {pageCount}</span><div className="flex gap-2"><Button variant="secondary" size="sm" onClick={() => onPageChange(page - 1)} disabled={page <= 1}>{t.common.uiPrevious}</Button><Button variant="secondary" size="sm" onClick={() => onPageChange(page + 1)} disabled={page >= pageCount}>{t.common.uiNext}</Button></div></nav>;
}

export function Button({ children, variant = 'primary', size = 'md', disabled, loading, className, type = 'button', ...props }: React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: 'primary' | 'secondary' | 'danger' | 'ghost'; size?: 'sm' | 'md' | 'lg'; loading?: boolean }) {
  const variants = { primary: 'bg-surface-900 text-white hover:bg-[#b8413a]', secondary: 'border border-surface-200 bg-white text-surface-700 hover:bg-surface-100 hover:text-surface-900', danger: 'bg-danger text-white hover:bg-danger-dark', ghost: 'text-surface-600 hover:bg-surface-100 hover:text-surface-900' };
  const sizes = { sm: 'px-2.5 py-1.5 text-xs', md: 'px-3 py-2 text-sm', lg: 'px-4 py-2.5 text-sm' };
  return <button type={type} className={join('inline-flex items-center justify-center gap-2 rounded-full font-medium transition-colors disabled:pointer-events-none disabled:opacity-50', variants[variant], sizes[size], className)} disabled={disabled || loading} aria-busy={loading || undefined} {...props}>{loading && <LoaderCircle size={15} className="animate-spin" aria-hidden="true" />}{children}</button>;
}
