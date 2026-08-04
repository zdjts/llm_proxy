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

  return <div className="min-h-screen bg-[#f6f7f9] text-surface-800">
    <Sidebar mobileOpen={mobileNavOpen} onMobileClose={closeMobileNav} />
    <div className="min-h-screen lg:pl-[248px]">
      <TopBar menuButtonRef={menuButtonRef} onMenuClick={openMobileNav} />
      <main className="min-w-0 px-4 py-5 sm:px-6 lg:px-9 lg:py-8"><div className="mx-auto max-w-[1440px]"><Outlet /></div></main>
    </div>
  </div>;
}
