import { useEffect, useRef, useState } from 'react';
import { Menu, X } from 'lucide-react';
import { Link, NavLink, Outlet, useLocation } from 'react-router-dom';
import { useLocale } from '@/i18n/context';

const leftNav = [
  { to: '/models', key: 'models' as const },
  { to: '/status', key: 'status' as const },
  { to: '/docs', key: 'documentation' as const },
  { to: '/about', key: 'about' as const },
  { to: '/contact', key: 'contact' as const },
];

export function PublicShell() {
  const { t, locale, setLocale } = useLocale();
  const [open, setOpen] = useState(false);
  const [scrolled, setScrolled] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const wasOpen = useRef(false);
  const location = useLocation();

  useEffect(() => { setOpen(false); }, [location.pathname]);
  useEffect(() => {
    if (!open && wasOpen.current) buttonRef.current?.focus();
    wasOpen.current = open;
  }, [open]);
  useEffect(() => {
    const close = (event: KeyboardEvent) => { if (event.key === 'Escape') setOpen(false); };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, []);
  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  const brand = (
    <Link className="brand" to="/">
      <span className="brand-mark brand-mark-fallback" aria-hidden="true">P</span>
      <span className="brand-name">llm_proxy</span>
    </Link>
  );

  const links = (mobile: boolean) => (
    <nav
      className={mobile ? 'page-nav page-nav-mobile' : 'page-nav page-nav-desktop'}
      aria-label={mobile ? t.site.nav.publicMobileNavigation : t.site.nav.publicNavigation}
    >
      {!mobile && (
        <div className="header-nav">
          {leftNav.map((item) => (
            <NavLink key={item.to} to={item.to} className="nav-link">
              {item.key === 'models' && t.site.nav.models}
              {item.key === 'status' && t.site.nav.status}
              {item.key === 'documentation' && t.site.nav.documentation}
              {item.key === 'about' && t.site.home.nav.about}
              {item.key === 'contact' && t.site.home.nav.contact}
            </NavLink>
          ))}
        </div>
      )}
      {mobile && leftNav.map((item) => (
        <NavLink key={item.to} to={item.to} className="nav-link">
          {item.key === 'models' && t.site.nav.models}
          {item.key === 'status' && t.site.nav.status}
          {item.key === 'documentation' && t.site.nav.documentation}
          {item.key === 'about' && t.site.home.nav.about}
          {item.key === 'contact' && t.site.home.nav.contact}
        </NavLink>
      ))}
      <button
        type="button"
        className="nav-link"
        aria-label={t.common.switchLang}
        onClick={() => setLocale(locale === 'zh-CN' ? 'en' : 'zh-CN')}
      >
        {locale === 'zh-CN' ? 'EN' : '中文'}
      </button>
      <Link className="nav-link" to="/login">{t.site.home.nav.signIn}</Link>
      <Link className="nav-cta" to="/login">{t.site.home.nav.console}</Link>
    </nav>
  );

  return (
    <div className="public-shell">
      <header className={`page-header ${scrolled ? 'is-scrolled' : ''}`}>
        <div className="page-container header-row">
          <div className="header-left">{brand}</div>
          {links(false)}
          <button
            ref={buttonRef}
            type="button"
            className="mobile-nav-toggle"
            aria-label={open ? t.site.nav.closeNavigation : t.site.nav.openNavigation}
            aria-expanded={open}
            onClick={() => setOpen((value) => !value)}
          >
            {open ? <X size={20} /> : <Menu size={20} />}
          </button>
        </div>
        {open && <div className="page-container">{links(true)}</div>}
      </header>
      <div className="public-shell-main">
        <Outlet />
      </div>
      <footer className="page-footer">
        <div className="page-container footer-row">
          <span className="f-brand">llm_proxy · {t.site.home.footer.tagline}</span>
          <span className="f-links">
            <Link to="/docs">{t.site.home.footer.docs}</Link>
            <Link to="/claude-code">{t.site.home.footer.claudeCode}</Link>
            <Link to="/codex">{t.site.home.footer.codex}</Link>
            <span className="f-copy">© {new Date().getFullYear()} llm_proxy</span>
          </span>
        </div>
      </footer>
    </div>
  );
}
