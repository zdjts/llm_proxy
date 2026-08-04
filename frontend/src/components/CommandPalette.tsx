import { useEffect, useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Search, LayoutDashboard, Server, Key, Boxes, Wallet, Settings, ShieldCheck } from 'lucide-react';

interface CommandItem {
  id: string;
  label: string;
  icon: React.ComponentType<{ size?: number; className?: string }>;
  to: string;
  keywords: string[];
}

const commands: CommandItem[] = [
  { id: 'overview', label: 'Overview', icon: LayoutDashboard, to: '/console', keywords: ['home', 'dashboard'] },
  { id: 'providers', label: 'Providers', icon: Server, to: '/config/providers', keywords: ['config', 'upstream'] },
  { id: 'pools', label: 'Key Pools', icon: Key, to: '/config/pools', keywords: ['keys', 'api'] },
  { id: 'routing', label: 'Routing', icon: Boxes, to: '/config/routing', keywords: ['model', 'route'] },
  { id: 'models', label: 'Model Catalog', icon: Boxes, to: '/config/models', keywords: ['catalog', 'registry'] },
  { id: 'budgets', label: 'Budgets', icon: Wallet, to: '/budgets', keywords: ['spend', 'cost'] },
  { id: 'users', label: 'Users', icon: Search, to: '/admin/users', keywords: ['admin', 'team'] },
  { id: 'audit', label: 'Audit Log', icon: ShieldCheck, to: '/settings/audit', keywords: ['log', 'history'] },
  { id: 'settings', label: 'System Settings', icon: Settings, to: '/settings/system', keywords: ['config', 'status'] },
  { id: 'help', label: 'Help', icon: Search, to: '/help', keywords: ['docs', 'runbook'] },
];

export function useCommandPalette() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const navigate = useNavigate();

  const filtered = query
    ? commands.filter(c => c.label.toLowerCase().includes(query.toLowerCase()) || c.keywords.some(k => k.includes(query.toLowerCase())))
    : commands;

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        setOpen(o => !o);
        setQuery('');
        setSelectedIndex(0);
      }
      if (e.key === 'Escape' && open) { setOpen(false); }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [open]);

  const execute = useCallback((item: CommandItem) => {
    setOpen(false);
    navigate(item.to);
  }, [navigate]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setSelectedIndex(i => Math.min(i + 1, filtered.length - 1)); }
    if (e.key === 'ArrowUp') { e.preventDefault(); setSelectedIndex(i => Math.max(i - 1, 0)); }
    if (e.key === 'Enter' && filtered[selectedIndex]) { execute(filtered[selectedIndex]); }
  };

  return { open, setOpen, query, setQuery, selectedIndex, filtered, execute, handleKeyDown };
}

export function CommandPalette() {
  const { open, query, setQuery, selectedIndex, filtered, execute, handleKeyDown, setOpen } = useCommandPalette();

  if (!open) return null;

  return (
    <>
      <div className="cmdk-overlay" onClick={() => setOpen(false)} />
      <div className="cmdk-palette">
        <div className="p-3 border-b border-surface-200">
          <div className="flex items-center gap-2 px-2">
            <Search size={16} className="text-surface-400 flex-shrink-0" />
            <input
              autoFocus
              value={query}
              onChange={e => setQuery(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Search pages... (Ctrl+K)"
              className="flex-1 bg-transparent border-none outline-none text-sm text-surface-700 placeholder:text-surface-300"
            />
            <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-100 text-surface-400 font-mono border border-surface-200">esc</kbd>
          </div>
        </div>
        <div className="p-2">
          {filtered.length === 0 && <p className="text-sm text-surface-400 text-center py-8">No results</p>}
          {filtered.map((item, i) => (
            <button
              key={item.id}
              onClick={() => execute(item)}
              className={`flex items-center gap-3 w-full px-3 py-2.5 rounded-lg text-sm text-left transition-colors ${i === selectedIndex ? 'bg-primary-50 text-primary-700' : 'text-surface-600 hover:bg-surface-50'}`}
            >
              <item.icon size={16} />
              {item.label}
            </button>
          ))}
        </div>
      </div>
    </>
  );
}
