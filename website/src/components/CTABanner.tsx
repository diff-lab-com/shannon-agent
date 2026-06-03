import { useState } from 'react';
import { type Lang, getTranslations } from '../i18n';

interface CTABannerProps {
  lang: Lang;
}

export default function CTABanner({ lang }: CTABannerProps) {
  const t = getTranslations(lang);
  const [copied, setCopied] = useState(false);

  const copyCommand = () => {
    navigator.clipboard.writeText(t.cta.command);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  return (
    <section style={{
      maxWidth: 'var(--max-width)',
      margin: '0 auto',
      padding: '0 24px 100px',
    }}>
      <div style={{
        background: 'var(--code-bg)',
        borderRadius: 'var(--radius-lg)',
        padding: '56px 40px',
        textAlign: 'center',
      }}>
        <h2 style={{ color: '#cdd6f4', marginBottom: 24, fontSize: 28 }}>{t.cta.title}</h2>
        <div style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 12,
          background: 'rgba(255,255,255,0.06)',
          borderRadius: 'var(--radius-sm)',
          padding: '12px 20px',
          marginBottom: 28,
          maxWidth: '100%',
          overflowX: 'auto',
        }}>
          <code style={{
            fontFamily: 'var(--font-mono)',
            fontSize: 14,
            color: '#a6adc8',
            whiteSpace: 'nowrap',
          }}>
            {t.cta.command}
          </code>
          <button
            onClick={copyCommand}
            style={{
              background: copied ? 'rgba(28, 154, 99, 0.2)' : 'rgba(255,255,255,0.08)',
              border: `1px solid ${copied ? 'rgba(28, 154, 99, 0.4)' : 'rgba(255,255,255,0.12)'}`,
              borderRadius: 'var(--radius-sm)',
              padding: '5px 12px',
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
              color: copied ? '#a6e3a1' : '#a6adc8',
              cursor: 'pointer',
              transition: 'all 0.15s',
              whiteSpace: 'nowrap',
            }}
          >
            {copied ? (lang === 'en' ? 'Copied!' : '已复制！') : (lang === 'en' ? 'Copy' : '复制')}
          </button>
        </div>
        <br />
        <a href="/docs" style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 8,
          padding: '10px 22px',
          background: 'rgba(255,255,255,0.1)',
          color: '#cdd6f4',
          border: '1px solid rgba(255,255,255,0.15)',
          borderRadius: 'var(--radius-sm)',
          fontSize: 15,
          fontWeight: 500,
          textDecoration: 'none',
          transition: 'background 0.15s',
        }}>
          {t.cta.button}
        </a>
      </div>
    </section>
  );
}
