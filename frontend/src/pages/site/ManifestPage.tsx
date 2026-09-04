import { Link } from 'react-router-dom';
import { ArrowRight, BookOpen, FileText, LockKeyhole, ShieldCheck } from 'lucide-react';
import { Card, Button } from '@/components/ui';
import { UnavailableState } from '@/components/site/UnavailableState';
import { useLocale } from '@/i18n/context';
import type { SiteRoute } from '@/routes/manifest';

function PublicUnavailable({ body }: { body: string }) {
  const { t } = useLocale();
  return (
    <Card className="mx-auto max-w-3xl py-12 text-center">
      <LockKeyhole className="mx-auto mb-4 text-surface-400" size={28} aria-hidden="true" />
      <h2 className="font-serif text-2xl font-bold text-surface-900">{t.site.actions.unavailable}</h2>
      <p className="mx-auto mt-2 max-w-xl text-sm leading-6 text-surface-500">{body}</p>
      <Button className="mt-6" disabled aria-disabled="true">{t.site.actions.unavailable}</Button>
    </Card>
  );
}

function AuthUnavailablePage({ route }: { route: SiteRoute }) {
  const { t } = useLocale();
  const copy = t.site.routes[route.key];
  return (
    <main className="auth-page auth-unavailable min-h-screen">
      <div className="auth-unavailable-inner">
        <Link className="public-back-link" to="/"><ArrowRight className="rotate-180" size={15} />{t.site.nav.product}</Link>
        <section className="auth-unavailable-card">
          <div className="auth-unavailable-mark">P</div>
          <p className="auth-unavailable-brand">llm_proxy</p>
          <h1>{copy.title}</h1>
          <p className="auth-unavailable-copy">{copy.description}</p>
          <LockKeyhole className="mx-auto mt-7 text-surface-400" size={26} aria-hidden="true" />
          <Button className="mt-6" disabled aria-disabled="true">{copy.action || t.site.actions.unavailable}</Button>
          <p className="auth-unavailable-foot">{t.site.public.unavailableBody}</p>
        </section>
      </div>
    </main>
  );
}

function DocsUnavailablePage({ route }: { route: SiteRoute }) {
  const { t } = useLocale();
  const copy = t.site.routes[route.key];
  return (
    <div className="docs-page">
      <header className="docs-topbar">
        <div className="docs-topbar-inner">
          <Link className="docs-back" to="/docs">← {t.site.routes.docs.title}</Link>
          <div className="docs-topbar-title">
            <span className="docs-topbar-eyebrow">DOCS</span>
            <span className="docs-topbar-cn">{copy.title}</span>
          </div>
          <Link className="docs-cta" to="/login">{t.site.home.nav.console}</Link>
        </div>
      </header>
      <div className="docs-shell">
        <aside className="docs-sidebar">
          <span className="docs-sidebar-overview is-active">{t.site.routes.docs.title}</span>
          <span className="docs-sidebar-page is-active">{copy.title}</span>
          <span className="docs-sidebar-page">{t.site.public.docsTopics[0]}</span>
          <span className="docs-sidebar-page">{t.site.public.docsTopics[1]}</span>
        </aside>
        <article className="docs-content">
          <div className="docs-hero">
            <p className="docs-hero-eyebrow">{t.site.hero.eyebrow}</p>
            <h1 className="docs-hero-title">{copy.title}</h1>
            <p>{copy.description}</p>
          </div>
          <div className="public-docs-placeholder">
            <LockKeyhole size={22} aria-hidden="true" />
            <span>{t.site.public.guideBody}</span>
          </div>
          <Button disabled aria-disabled="true">{t.site.actions.unavailable}</Button>
        </article>
      </div>
    </div>
  );
}

function HomePage() {
  const { t } = useLocale();
  const h = t.site.home;
  const eyebrowBits = h.hero.eyebrow.split('·').map((part) => part.trim());

  return (
    <div className="home-page">
      <section className="hero-section">
        <div className="page-container hero-block">
          <p className="hero-eyebrow">
            {eyebrowBits.map((bit, index) => (
              <span key={bit} style={{ ['--ebi' as string]: index }}>
                {index > 0 && <span className="eb-dot">·</span>}
                <span className={`eb-bit ${index === 0 ? 'eb-no' : index === eyebrowBits.length - 1 ? 'eb-em' : 'eb-strike'}`}>{bit}</span>
              </span>
            ))}
          </p>
          <h1 className="hero-title">{t.site.routes.home.title}</h1>
          <div className="hero-zh" aria-hidden="true">
            <span className="hz-brand">{h.hero.titleParts.brand}</span>
            <span className="hz-mid">{h.hero.titleParts.mid}</span>
          </div>
          <p className="hero-en">{h.hero.titleParts.tail} · API Gateway</p>
          <p className="hero-slogan">{h.hero.tagline}</p>
          <p style={{ margin: 0, maxWidth: '46ch', color: '#525252', fontSize: 15, lineHeight: 1.8 }}>{h.hero.sub}</p>
          <div className="hero-ctas">
            <Link className="cta-primary" to="/login">{h.cta.start}<span className="arrow">→</span></Link>
            <Link className="cta-text" to="/docs">{h.cta.docs}<span className="arrow-tiny">↗</span></Link>
          </div>
          <ul className="active-on">
            <li className="active-on-label">{h.hero.activeOn}</li>
            {h.hero.works.map((item) => <li key={item}>{item}</li>)}
          </ul>
        </div>
        <a className="hero-scroll-cue" href="#manifesto" aria-label="scroll">
          <span className="scroll-track"><span className="scroll-dot" /></span>
        </a>
      </section>

      <section id="manifesto" className="manifesto-section in-view">
        <div className="page-container manifesto-block">
          <p className="manifesto-tag">{h.manifesto.tag}</p>
          <h2 className="manifesto-title has-token-play">
            <span className="manifesto-sweep" aria-hidden="true" />
            {h.manifesto.title}
          </h2>
          <div className="integrity-check" style={{ opacity: 1 }}>
            <span className="ic-hash"><span className="ic-hash-label">SHA</span><span className="ic-hash-val">a3f9e2c7…d41b7c</span></span>
            <span className="ic-verified" style={{ opacity: 1 }}><span className="ic-verified-label">{h.manifesto.integrity}</span></span>
          </div>
          <div className="manifesto-body">
            <p>{h.manifesto.body1}</p>
            <p>{h.manifesto.body2}</p>
          </div>
          <ul className="manifesto-pledges">
            {h.manifesto.pledges.map((pledge) => (
              <li key={pledge.label} className="pledge" tabIndex={0}>
                <span className="pledge-mark" aria-hidden="true">✦</span>
                <span className="pledge-label">{pledge.label}</span>
                <div className="pledge-card">
                  <p className="pledge-card-title">{pledge.title}</p>
                  <p className="pledge-card-desc">{pledge.desc}</p>
                </div>
              </li>
            ))}
          </ul>
        </div>
      </section>

      <section className="stats-section in-view">
        <div className="page-container">
          <div className="stats-strip">
            <div className="stat">
              <div className="stat-value"><span className="od-static">12.8</span><span className="stat-unit">M+</span></div>
              <div className="stat-label">{h.stats.requests}</div>
            </div>
            <div className="stat">
              <div className="stat-value"><span className="od-static">99.9</span><span className="stat-unit">%</span></div>
              <div className="stat-label">{h.stats.uptime}</div>
            </div>
            <div className="stat">
              <div className="stat-value"><span className="od-static">&lt;100</span><span className="stat-unit">ms</span></div>
              <div className="stat-label">{h.stats.latency}</div>
            </div>
          </div>
        </div>
      </section>

      <section className="image-section">
        <div className="page-container section-block">
          <div className="section-head">
            <span className="section-tag">{h.sections.imageTag}</span>
            <h2 className="section-title">{h.sections.imageTitle}</h2>
            <p className="section-lede">{h.sections.imageLede}</p>
          </div>
          <div className="image-static">
            <div className="image-static-card">
              <span className="section-tag">{h.image.badge}</span>
              <h3 className="section-title" style={{ fontSize: 28 }}>{h.image.model}</h3>
              <p style={{ color: '#525252', lineHeight: 1.7 }}>{h.image.desc}</p>
              <ul className="home-works" style={{ justifyContent: 'flex-start', marginTop: 18 }}>
                {h.image.caps.map((cap) => <li key={cap}>{cap}</li>)}
              </ul>
              <button type="button" className="img-doclink inert-btn" aria-disabled="true" disabled>{h.image.docLink}<span className="img-doclink-arrow">→</span></button>
            </div>
            <div className="image-static-dark">
              <div>
                <div className="feature-en">PROMPT</div>
                <p style={{ marginTop: 10, fontFamily: 'JetBrains Mono, monospace', fontSize: 13, lineHeight: 1.7 }}>A quiet coastal sunset, volumetric light, photoreal</p>
              </div>
              <div className="feature-en">1024×1024 · demo surface</div>
            </div>
          </div>
        </div>
      </section>

      <section className="channels-section">
        <div className="page-container section-block">
          <div className="section-head">
            <span className="section-tag">{h.channels.tag}</span>
            <h2 className="section-title">{h.channels.title}</h2>
          </div>
          <div className="channels-layout">
            <div className="channels-copy">
              <span className="chc-rule" />
              <h3 className="chc-title">{h.channels.copyTitle}</h3>
              <p className="chc-body">{h.channels.copyBody}</p>
            </div>
            <div className="channels-static">
              {h.channels.items.map((item) => (
                <div key={item.name} className="channel-chip">
                  <strong>{item.name}</strong>
                  <span>{item.hint}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section className="features-section in-view">
        <div className="page-container section-block">
          <div className="section-head">
            <span className="section-tag">{h.sections.featuresTag}</span>
            <h2 className="section-title">{h.sections.featuresTitle}</h2>
          </div>
          <ul className="why-ledger">
            {h.features.map((feature, index) => (
              <li key={feature.title} className="why-row" style={{ ['--d' as string]: `${index * 80}ms` }}>
                <span className="why-idx">0{index + 1}</span>
                <div className="why-head">
                  <h3 className="why-title">{feature.title}</h3>
                  <span className="why-en">{feature.en}</span>
                </div>
                <p className="why-desc">{feature.desc}</p>
              </li>
            ))}
          </ul>
        </div>
      </section>

      <section className="code-section">
        <div className="page-container section-block">
          <div className="section-head">
            <span className="section-tag">{h.sections.codeTag}</span>
            <h2 className="section-title">{h.sections.codeTitle}</h2>
            <p className="section-lede">{h.sections.codeLede}</p>
          </div>
          <div className="onboard-grid">
            <ol className="onboard-steps">
              {[{ t: h.onboard.s1t, d: h.onboard.s1d }, { t: h.onboard.s2t, d: h.onboard.s2d }, { t: h.onboard.s3t, d: h.onboard.s3d }].map((step, index) => (
                <li key={step.t} className={`onboard-step ${index === 0 ? 'is-now' : ''}`}>
                  <span className="onboard-no">{index + 1}</span>
                  <div className="onboard-step-body">
                    <h3 className="onboard-step-title">{step.t}</h3>
                    <p className="onboard-step-desc">{step.d}</p>
                  </div>
                </li>
              ))}
            </ol>
            <div className="image-static-card">
              <p className="onboard-foot">{h.onboard.docLink} <Link className="onboard-foot-link" to="/docs">{h.onboard.docLinkCta}</Link></p>
            </div>
          </div>
        </div>
      </section>

      <section className="pricing-section">
        <div className="page-container section-block">
          <span className="section-tag">{h.sections.pricingTag}</span>
          <div className="pricing-headline">
            <span>{h.pricing.lineA}</span>
            <span className="pricing-line2">{h.pricing.lineB}</span>
            <span>{h.pricing.lineC}</span>
          </div>
          <p className="pricing-blurb">{h.pricing.blurb}</p>
          <ul className="pricing-tags">
            {h.pricing.tags.map((tag) => <li key={tag}>— {tag}</li>)}
          </ul>
          <Link className="cta-text-large" to="/models">{h.cta.viewPrice}<span className="arrow">→</span></Link>
        </div>
      </section>

      <section className="closer-section">
        <div className="closer-stage">
          <div className="closer-overlay">
            <div className="closer-logo brand-mark-fallback" aria-hidden="true">P</div>
            <h2 className="closer-title">{h.closer.title}</h2>
            <p className="closer-sub">{h.closer.sub}</p>
            <Link className="cta-primary" to="/login">{h.cta.start}<span className="arrow">→</span></Link>
          </div>
        </div>
      </section>
    </div>
  );
}

function AboutPage() {
  const { t } = useLocale();
  return (
    <div className="about-page">
      <header className="about-header">
        <Link className="back-link" to="/">{t.site.public.backHome}</Link>
        <p className="about-eyebrow">{t.site.hero.eyebrow}</p>
        <h1 className="about-title">{t.site.routes.about.title}</h1>
        <p className="about-lede">{t.site.public.aboutBody}</p>
      </header>
      <main className="about-main">
        <section className="about-section">
          <p className="about-section-num">01</p>
          <h2 className="about-section-title">{t.site.hero.operationalTitle}</h2>
          <p>{t.site.hero.operationalDescription}</p>
        </section>
        <section className="about-section about-section--accent">
          <p className="about-section-num">02</p>
          <h2 className="about-section-title">{t.site.routes.models.title}</h2>
          <p>{t.site.public.modelsBody}</p>
          <p className="about-verify">{t.site.public.unavailableBody}</p>
        </section>
        <section className="about-section">
          <p className="about-section-num">03</p>
          <h2 className="about-section-title">{t.site.home.manifesto.tag}</h2>
          <p>{t.site.home.manifesto.body1}</p>
          <p>{t.site.home.manifesto.body2}</p>
        </section>
        <div className="about-cta">
          <p>{t.site.routes.contact.description}</p>
          <div className="about-cta-row">
            <Link className="about-cta-btn about-cta-btn--primary" to="/login">{t.site.home.cta.console}</Link>
            <Link className="about-cta-btn" to="/contact">{t.site.home.nav.contact}</Link>
          </div>
        </div>
      </main>
    </div>
  );
}

function ModelsPage() {
  const { t } = useLocale();
  const rows = [
    { name: 'Claude Opus', input: '—', output: '—', cache: '—' },
    { name: 'Claude Sonnet', input: '—', output: '—', cache: '—' },
    { name: 'GPT family', input: '—', output: '—', cache: '—' },
    { name: 'Gemini family', input: '—', output: '—', cache: '—' },
  ];
  return (
    <div className="models-page">
      <header className="mp-header">
        <Link className="back-link" to="/">{t.site.public.backHome}</Link>
      </header>
      <main className="mp-main">
        <p className="mp-tag">MODELS & PRICING</p>
        <h1 className="mp-title">{t.site.routes.models.title}</h1>
        <p className="mp-desc">{t.site.public.modelsBody}</p>
        <h2 className="mp-subtitle">{t.site.public.modelsSubtitle}</h2>
        <div className="mp-table-wrap">
          <table className="mp-table">
            <thead>
              <tr>
                <th>{t.site.public.modelName}</th>
                <th>{t.site.public.modelInput}</th>
                <th>{t.site.public.modelOutput}</th>
                <th>{t.site.public.modelCacheRead}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.name}>
                  <td className="mp-model">{row.name}</td>
                  <td>{row.input}</td>
                  <td>{row.output}</td>
                  <td>{row.cache}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <p className="mp-note">{t.site.public.modelsNote}</p>
        <p className="mp-note">{t.site.public.modelsNote2}</p>
        <p className="mp-links">
          <Link to="/claude-code">Claude Code</Link>
          <span className="mp-dot">·</span>
          <Link to="/codex">Codex</Link>
          <span className="mp-dot">·</span>
          <Link to="/docs">{t.site.routes.docs.title}</Link>
        </p>
        <Link className="mp-cta" to="/login">{t.site.home.cta.console} →</Link>
      </main>
    </div>
  );
}

function StatusPage({ route }: { route: SiteRoute }) {
  const { t } = useLocale();
  const copy = t.site.routes[route.key];
  const services = ['Chat Completions', 'Models API', 'Admin Console', 'WebSocket Live'];
  return (
    <div className="sst-page">
      <div className="sst-container">
        <header className="sst-header">
          <p className="sst-eyebrow">STATUS</p>
          <h1 className="sst-title">{copy.title}</h1>
          <p className="sst-subtitle">{copy.description}</p>
        </header>
        <section className="sst-hero">
          <div className="sst-hero-main">
            <span className="sst-hero-dot is-empty" />
            <div>
              <div className="sst-hero-status">{t.site.public.statusAllOk}</div>
              <div className="sst-hero-en">{t.site.public.statusAllOkEn}</div>
            </div>
          </div>
          <div className="sst-hero-meta">
            <span>{t.site.public.statusBody}</span>
          </div>
        </section>
        <section className="sst-section">
          <div className="sst-section-head">
            <h2 className="sst-section-title">{t.site.public.statusCoreServices}</h2>
            <span className="sst-section-note">{t.site.public.statusLast30d}</span>
          </div>
          <div className="sst-services">
            {services.map((name) => (
              <div key={name} className="sst-service-row">
                <div className="sst-service-name"><span className="sst-dot is-empty" />{name}</div>
                <div className="sst-service-band-wrap"><div className="sst-service-band" style={{ background: '#e4e4df' }} /></div>
                <div className="sst-service-uptime sst-mono">—</div>
              </div>
            ))}
          </div>
        </section>
        <section className="sst-section">
          <div className="sst-stats">
            {[t.site.public.statusOperational, t.site.public.statusUnavailable, t.site.public.statusBody].map((label) => (
              <div key={label} className="sst-stat">
                <div className="sst-stat-num sst-mono">—</div>
                <div>{label}</div>
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}

function ContactPage({ support = false }: { support?: boolean }) {
  const { t } = useLocale();
  return (
    <div className="contact-page">
      <header className="contact-header">
        <Link className="back-link" to="/">{t.site.public.backHome}</Link>
      </header>
      <main className="contact-main">
        <p className="contact-eyebrow">{t.site.public.contactEyebrow}</p>
        <h1 className="contact-title">{t.site.routes[support ? 'contactSupport' : 'contact'].title}</h1>
        <p className="contact-desc">{support ? t.site.public.contactSupportBody : t.site.public.contactBody}</p>
        <div className="contact-grid">
          <div className="contact-card" aria-disabled="true">
            <span className="contact-card-glyph">@</span>
            <div className="contact-card-body">
              <h2 className="contact-card-name">{support ? t.site.routes.contactSupport.title : t.site.routes.contact.title}</h2>
              <p className="contact-card-handle">support@local</p>
              <p className="contact-card-desc">{t.site.public.contactSupportBody}</p>
            </div>
            <span className="contact-card-cta">{t.site.public.contactCardCta}<ArrowRight size={14} /></span>
          </div>
          <div className="contact-card" style={{ background: '#fafaf7' }} aria-disabled="true">
            <span className="contact-card-glyph">?</span>
            <div className="contact-card-body">
              <h2 className="contact-card-name">{t.site.public.statusUnavailable}</h2>
              <p className="contact-card-handle">—</p>
              <p className="contact-card-desc">{t.site.public.unavailableBody}</p>
            </div>
            <span className="contact-card-cta">{t.site.actions.unavailable}</span>
          </div>
        </div>
        <p className="contact-footnote">{t.site.public.unavailableBody}</p>
      </main>
    </div>
  );
}

function DocsPage() {
  const { t } = useLocale();
  return (
    <div className="docs-page">
      <header className="docs-topbar">
        <div className="docs-topbar-inner">
          <Link className="docs-back" to="/">{t.site.public.backHome}</Link>
          <div className="docs-topbar-title">
            <span className="docs-topbar-eyebrow">DOCUMENTATION</span>
            <span className="docs-topbar-cn">{t.site.routes.docs.title}</span>
          </div>
          <Link className="docs-cta" to="/login">{t.site.home.nav.console}</Link>
        </div>
      </header>
      <div className="docs-shell">
        <aside className="docs-sidebar">
          <span className="docs-sidebar-overview is-active">{t.site.routes.docs.title}</span>
          {t.site.public.docsTopics.map((topic) => (
            <span key={topic} className="docs-sidebar-page">{topic}</span>
          ))}
        </aside>
        <div className="docs-content">
          <div className="docs-hero">
            <p className="docs-hero-eyebrow">{t.site.hero.eyebrow}</p>
            <h1 className="docs-hero-title">{t.site.routes.docs.title}</h1>
            <p>{t.site.public.docsBody}</p>
          </div>
          <div className="grid gap-4 md:grid-cols-3">
            {t.site.public.docsTopics.map((topic) => (
              <Card key={topic}>
                <BookOpen size={20} className="mb-4 text-surface-800" aria-hidden="true" />
                <h2 className="font-semibold text-surface-900">{topic}</h2>
                <p className="mt-2 text-sm text-surface-500">{t.site.public.docsBody}</p>
              </Card>
            ))}
          </div>
          <Card className="mt-6">
            <div className="flex items-center gap-3 text-sm text-surface-600">
              <FileText size={18} aria-hidden="true" />
              {t.site.public.unavailableBody}
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}

const accountPresentationKeys = new Set(['accountOverview', 'profile', 'usage', 'requestHistory', 'apiKeys', 'keyUsage', 'availableModels', 'availableChannels', 'groups', 'accountSettings', 'announcements', 'auditEvents', 'errorEvents']);
const inertAccountFamilyGroups = new Set(['commercial', 'community', 'activities']);
const operatorRouteKeys = new Set(['operationsDashboard', 'operatorUsers', 'operatorAccounts', 'operatorGroups', 'operatorChannels', 'operatorOrders', 'paymentDashboard', 'operatorPaymentPlans', 'affiliateInvites', 'affiliateRebates', 'affiliateTransfers', 'affiliateRecords', 'agentApplications', 'agentConfiguration', 'agentList', 'agentWithdrawals', 'operatorAuditLog', 'operatorErrorLog']);

function AccountPresentationPage({ route }: { route: SiteRoute }) {
  const { t } = useLocale();
  const copy = t.site.routes[route.key];
  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <p className="text-xs font-medium uppercase tracking-wide text-surface-500">{t.site.account.groups[route.group as 'workspace' | 'developer']}</p>
          <h1 className="page-title mt-2">{copy.title}</h1>
          <p className="page-subtitle mt-3 max-w-2xl">{copy.description}</p>
        </div>
        <Button disabled aria-disabled="true">{route.key === 'auditEvents' || route.key === 'requestHistory' ? t.site.account.exportAction : t.site.account.editAction}</Button>
      </div>
      <Card>
        <p className="text-sm leading-6 text-surface-500">{t.site.account.overviewBody}</p>
        <div className="mt-6 grid grid-cols-2 border-y border-surface-200 py-3 text-xs font-medium uppercase text-surface-500">
          <span>{t.site.account.tableField}</span>
          <span>{t.site.account.tableState}</span>
        </div>
        <div className="py-10 text-center text-sm text-surface-500">{t.site.account.emptyState}</div>
      </Card>
      <PublicUnavailable body={t.site.account.unavailableBody} />
    </div>
  );
}

function InertAccountFamilyPage({ route }: { route: SiteRoute }) {
  const { t } = useLocale();
  const copy = t.site.routes[route.key];
  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <p className="text-xs font-medium uppercase tracking-wide text-surface-500">{t.site.account.groups[route.group as 'commercial' | 'community' | 'activities']}</p>
          <h1 className="page-title mt-2">{copy.title}</h1>
          <p className="page-subtitle mt-3 max-w-2xl">{copy.description}</p>
        </div>
        <Button disabled aria-disabled="true">{t.site.account.familyAction}</Button>
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <Card>
          <h2 className="font-semibold text-surface-900">{t.site.account.familyTitle}</h2>
          <p className="mt-2 text-sm leading-6 text-surface-500">{t.site.account.familyBody}</p>
        </Card>
        <Card>
          <div className="grid grid-cols-2 border-b border-surface-200 pb-3 text-xs font-medium uppercase text-surface-500">
            <span>{t.site.account.tableField}</span>
            <span>{t.site.account.tableState}</span>
          </div>
          <p className="py-8 text-center text-sm text-surface-500">{t.site.account.familyState}</p>
        </Card>
      </div>
      <PublicUnavailable body={t.site.account.unavailableBody} />
    </div>
  );
}

function OperatorPage({ route }: { route: SiteRoute }) {
  const { t } = useLocale();
  const copy = t.site.routes[route.key];
  return (
    <div className="space-y-6">
      <div>
        <p className="text-xs font-medium uppercase tracking-wide text-surface-500">{t.site.account.groups.operator}</p>
        <h1 className="page-title mt-2">{copy.title}</h1>
        <p className="page-subtitle mt-3 max-w-2xl">{copy.description}</p>
      </div>
      <Card>
        <div className="flex flex-wrap gap-3">
          <input aria-label={t.site.account.operatorSearch} disabled placeholder={t.site.account.operatorSearch} className="input-glass min-w-0 flex-1" />
          <Button disabled aria-label={t.site.account.operatorFilter}>{t.site.account.operatorFilter}</Button>
          <Button disabled aria-label={t.site.account.operatorCreate}>{t.site.account.operatorCreate}</Button>
          <Button disabled aria-label={t.site.account.operatorExport}>{t.site.account.operatorExport}</Button>
        </div>
        <div className="mt-6 grid grid-cols-2 border-y border-surface-200 py-3 text-xs font-medium uppercase text-surface-500">
          <span>{t.site.account.tableField}</span>
          <span>{t.site.account.tableState}</span>
        </div>
        <p className="py-12 text-center text-sm text-surface-500">{t.site.account.familyState}</p>
      </Card>
      <PublicUnavailable body={t.site.account.operatorBody} />
    </div>
  );
}

export function ManifestPage({ route }: { route: SiteRoute }) {
  const { t } = useLocale();
  const copy = t.site.routes[route.key] || t.site.fallback;
  if (route.key === 'home') return <HomePage />;
  if (route.key === 'about') return <AboutPage />;
  if (route.key === 'models') return <ModelsPage />;
  if (route.key === 'status' || route.key === 'networkStatus') return <StatusPage route={route} />;
  if (route.key === 'contact') return <ContactPage />;
  if (route.key === 'contactSupport') return <ContactPage support />;
  if (route.key === 'docs') return <DocsPage />;
  if (accountPresentationKeys.has(route.key)) return <AccountPresentationPage route={route} />;
  if (operatorRouteKeys.has(route.key)) return <OperatorPage route={route} />;
  if (inertAccountFamilyGroups.has(route.group || '')) return <InertAccountFamilyPage route={route} />;
  if (route.key === 'register' || route.key === 'forgotPassword') return <AuthUnavailablePage route={route} />;
  if (route.key === 'batchImage' || route.key === 'claudeCode' || route.key === 'codex') return <DocsUnavailablePage route={route} />;
  if (route.key === 'modelPlaza') return <PublicUnavailable body={t.site.public.unavailableBody} />;
  if (route.mode === 'unavailable') return <UnavailableState title={copy.title} description={copy.description} action={copy.action} />;
  const Icon = route.icon;
  return (
    <section className="grid gap-8 py-6 lg:grid-cols-[1.2fr_0.8fr] lg:py-14">
      <div>
        <p className="mb-3 text-xs font-medium uppercase tracking-wide text-surface-500">{t.site.hero.eyebrow}</p>
        <h1 className="max-w-2xl font-serif text-3xl font-bold leading-tight text-surface-900 sm:text-4xl">{copy.title}</h1>
        <p className="mt-4 max-w-2xl text-base leading-7 text-surface-600">{copy.description}</p>
      </div>
      <Card className="flex min-h-52 flex-col justify-between">
        <Icon size={28} className="text-surface-800" aria-hidden="true" />
        <div>
          <h2 className="font-semibold text-surface-900">{t.site.hero.operationalTitle}</h2>
          <p className="mt-2 text-sm leading-6 text-surface-500">{t.site.hero.operationalDescription}</p>
        </div>
      </Card>
    </section>
  );
}
