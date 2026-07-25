import { useState, useEffect } from 'react';
import { Outlet, useLocation } from 'react-router-dom';
import { motion, AnimatePresence } from 'framer-motion';
import { Sidebar } from './Sidebar';
import { TopBar } from './TopBar';
import { ParticleBackground } from './ParticleBackground';
import { useLocale } from '@/i18n/context';

export function Layout() {
  const [collapsed, setCollapsed] = useState(false);
  const [bgIndex, setBgIndex] = useState(() => {
    const saved = localStorage.getItem('dashboard-bg-index');
    return saved ? parseInt(saved) : 0;
  });

  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'b') { e.preventDefault(); setCollapsed(c => !c); }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, []);

  const toggleBg = () => {
    const next = (bgIndex + 1) % 3;
    setBgIndex(next);
    localStorage.setItem('dashboard-bg-index', String(next));
  };

  const { t } = useLocale();

  return (
    <div className="relative min-h-screen bg-gradient-to-br from-surface-50 via-white to-primary-50/30">
      <ParticleBackground variant={bgIndex} />
      <div className="relative z-10 flex">
        <Sidebar collapsed={collapsed} onToggle={() => setCollapsed(c => !c)} />
        <div className={`flex-1 flex flex-col transition-all duration-300 min-h-screen ${collapsed ? 'ml-16' : 'ml-56'}`}>
          <TopBar />
          <main className="flex-1 p-6">
            <div className="max-w-[1600px] mx-auto">
              <AnimatePresence mode="wait">
                <motion.div
                  key={location.pathname}
                  initial={{ opacity: 0, y: 20, scale: 0.98 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: -10, scale: 0.98 }}
                  transition={{ duration: 0.3 }}
                >
                  <Outlet />
                </motion.div>
              </AnimatePresence>
            </div>
          </main>
        </div>
      </div>
      <button onClick={toggleBg} className="fixed bottom-4 right-4 z-50 w-8 h-8 rounded-full glass flex items-center justify-center text-xs text-primary-400 hover:text-primary-600 transition-colors" title={t.sidebar.toggleBg}>BG</button>
    </div>
  );
}
