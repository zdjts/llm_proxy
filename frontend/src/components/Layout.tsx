import { useEffect, useRef, useState } from 'react';
import { Outlet, useLocation } from 'react-router-dom';
import { Sidebar } from './Sidebar';
import { TopBar } from './TopBar';

export function Layout() {
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const menuButtonRef = useRef<HTMLButtonElement>(null);
  const wasMobileNavOpen = useRef(false);
  const location = useLocation();

  const openMobileNav = () => setMobileNavOpen(true);
  const closeMobileNav = () => setMobileNavOpen(false);

  useEffect(() => { setMobileNavOpen(false); }, [location.pathname]);
  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => { if (event.key === 'Escape') closeMobileNav(); };
    window.addEventListener('keydown', closeOnEscape);
    return () => window.removeEventListener('keydown', closeOnEscape);
  }, []);
  useEffect(() => {
    if (!mobileNavOpen && wasMobileNavOpen.current) menuButtonRef.current?.focus();
    wasMobileNavOpen.current = mobileNavOpen;
  }, [mobileNavOpen]);

  return <div className="min-h-screen bg-[#f7f6f2] text-surface-800">
    <Sidebar mobileOpen={mobileNavOpen} onMobileClose={closeMobileNav} />
    <div className="min-h-screen lg:pl-64">
      <TopBar menuButtonRef={menuButtonRef} onMenuClick={openMobileNav} />
      <main className="min-w-0 px-4 py-6 sm:px-6 lg:px-10 lg:py-8"><div className="mx-auto max-w-[1480px]"><Outlet /></div></main>
    </div>
  </div>;
}
