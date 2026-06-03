import { useState, useEffect } from 'react';
import { type Lang, getStoredLang, setStoredLang } from '../i18n';
import Navbar from './Navbar';
import Hero from './Hero';
import Terminal from './Terminal';
import FeatureGrid from './FeatureGrid';
import ComparisonTable from './ComparisonTable';
import CTABanner from './CTABanner';

export default function LandingPage() {
  const [lang, setLang] = useState<Lang>('en');

  useEffect(() => {
    setLang(getStoredLang());
  }, []);

  const handleLangChange = (next: Lang) => {
    setStoredLang(next);
    setLang(next);
  };

  return (
    <>
      <Navbar lang={lang} onLangChange={handleLangChange} />
      <Hero lang={lang} />
      <Terminal lang={lang} />
      <FeatureGrid lang={lang} />
      <ComparisonTable lang={lang} />
      <CTABanner lang={lang} />
      <footer style={{
        borderTop: '1px solid var(--line)',
        padding: '24px',
        textAlign: 'center',
        color: 'var(--muted)',
        fontSize: 14,
      }}>
        <p>{lang === 'en' ? '\u00a9 2026 Shannon Code Contributors.' : '\u00a9 2026 Shannon Code \u8d21\u732e\u8005\u3002'} Apache-2.0</p>
      </footer>
    </>
  );
}
