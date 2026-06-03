import { useState, useEffect } from 'react';
import { type Lang, setStoredLang, getTranslations, BASE } from '../i18n';

interface NavbarProps {
  lang: Lang;
  onLangChange: (lang: Lang) => void;
}

export default function Navbar({ lang, onLangChange }: NavbarProps) {
  const t = getTranslations(lang);
  const [scrolled, setScrolled] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  const toggleLang = () => {
    const next = lang === 'en' ? 'zh' : 'en';
    setStoredLang(next);
    onLangChange(next);
  };

  const navLinkStyle = {
    color: 'var(--muted)',
    fontSize: 15,
    textDecoration: 'none',
  } as const;

  return (
    <nav style={{
      position: 'sticky',
      top: 0,
      zIndex: 100,
      height: 'var(--nav-height)',
      display: 'flex',
      alignItems: 'center',
      background: scrolled ? 'rgba(15, 15, 20, 0.92)' : 'var(--bg)',
      backdropFilter: scrolled ? 'saturate(1.4) blur(12px)' : 'none',
      borderBottom: scrolled ? '1px solid var(--line)' : '1px solid transparent',
      transition: 'background 0.2s, border-color 0.2s, backdrop-filter 0.2s',
    }}>
      <div style={{
        maxWidth: 'var(--max-width)',
        margin: '0 auto',
        padding: '0 24px',
        width: '100%',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
      }}>
        <a href={BASE} style={{ display: 'flex', alignItems: 'center', gap: 10, color: 'var(--ink)', textDecoration: 'none' }}>
          <div style={{
            width: 28,
            height: 28,
            borderRadius: 7,
            background: 'var(--brand-gradient)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            color: 'white',
            fontFamily: 'var(--font-mono)',
            fontSize: 14,
            fontWeight: 700,
          }}>S</div>
          <span style={{ fontWeight: 600, fontSize: 16 }}>Shannon Code</span>
        </a>

        <div className={`nav-links${menuOpen ? ' open' : ''}`} style={{
          display: 'flex',
          alignItems: 'center',
          gap: 24,
        }}>
          <a href="#features" style={navLinkStyle} onClick={() => setMenuOpen(false)}>{t.nav.features}</a>
          <a href={BASE + 'docs'} style={navLinkStyle} onClick={() => setMenuOpen(false)}>{t.nav.docs}</a>
          <button
            onClick={toggleLang}
            style={{
              background: 'none',
              border: '1px solid var(--line)',
              borderRadius: 'var(--radius-sm)',
              padding: '4px 12px',
              fontFamily: 'var(--font-mono)',
              fontSize: 13,
              color: 'var(--muted)',
              cursor: 'pointer',
            }}
          >
            {lang === 'en' ? '中文' : 'EN'}
          </button>
          <a href="https://github.com/shannon-agent/shannon-code" target="_blank" rel="noopener noreferrer"
            style={navLinkStyle}>{t.nav.github}</a>
          <a href="#install" className="btn-primary" style={{ fontSize: 14, padding: '8px 18px' }}
            onClick={() => setMenuOpen(false)}>{t.nav.getStarted}</a>
        </div>

        {/* Hamburger */}
        <button
          className="nav-hamburger"
          onClick={() => setMenuOpen(o => !o)}
          style={{
            display: 'none',
            flexDirection: 'column',
            gap: 5,
            background: 'none',
            border: 'none',
            cursor: 'pointer',
            padding: 8,
          }}
          aria-label="Menu"
        >
          <span style={{ width: 22, height: 2, background: 'var(--ink)', borderRadius: 1, transition: 'transform 0.2s', transform: menuOpen ? 'rotate(45deg) translate(5px, 5px)' : 'none' }} />
          <span style={{ width: 22, height: 2, background: 'var(--ink)', borderRadius: 1, opacity: menuOpen ? 0 : 1, transition: 'opacity 0.2s' }} />
          <span style={{ width: 22, height: 2, background: 'var(--ink)', borderRadius: 1, transition: 'transform 0.2s', transform: menuOpen ? 'rotate(-45deg) translate(5px, -5px)' : 'none' }} />
        </button>
      </div>
    </nav>
  );
}
