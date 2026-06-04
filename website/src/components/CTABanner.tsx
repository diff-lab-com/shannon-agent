import { type Lang, getTranslations, BASE } from '../i18n';

interface CTABannerProps {
  lang: Lang;
}

export default function CTABanner({ lang }: CTABannerProps) {
  const t = getTranslations(lang);

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
        <h2 style={{ color: '#cdd6f4', marginBottom: 28, fontSize: 28 }}>{t.cta.title}</h2>
        <a href={BASE + 'docs/getting-started'} style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 8,
          padding: '12px 28px',
          background: 'rgba(255,255,255,0.1)',
          color: '#cdd6f4',
          border: '1px solid rgba(255,255,255,0.15)',
          borderRadius: 'var(--radius-sm)',
          fontSize: 15,
          fontWeight: 500,
          textDecoration: 'none',
          transition: 'background 0.15s',
        }}>
          {t.cta.button} →
        </a>
      </div>
    </section>
  );
}
