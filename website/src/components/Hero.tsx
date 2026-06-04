import { type Lang, getTranslations, BASE } from '../i18n';

interface HeroProps {
  lang: Lang;
}

export default function Hero({ lang }: HeroProps) {
  const t = getTranslations(lang);

  return (
    <section style={{
      textAlign: 'center',
      padding: '80px 24px 60px',
    }}>
      {/* Badge */}
      <div style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 8,
        padding: '6px 16px',
        background: 'var(--bg-soft)',
        border: '1px solid var(--line)',
        borderRadius: 'var(--radius-pill)',
        fontFamily: 'var(--font-mono)',
        fontSize: 13,
        color: 'var(--muted)',
        marginBottom: 32,
      }}>
        <span style={{
          width: 7,
          height: 7,
          borderRadius: '50%',
          background: 'var(--ok)',
          boxShadow: '0 0 0 3px rgba(52, 211, 153, 0.15)',
        }} />
        {t.hero.badge}
      </div>

      {/* Title */}
      <h1 style={{ maxWidth: 680, margin: '0 auto 24px' }}>
        {t.hero.title.map((part, i) => {
          if (typeof part === 'string') return <span key={i}>{part}</span>;
          return <em key={i} className="serif-italic">{part.text}</em>;
        })}
      </h1>

      {/* Subtitle */}
      <p style={{
        maxWidth: 600,
        margin: '0 auto 16px',
        color: 'var(--muted)',
        fontSize: 18,
        lineHeight: 1.6,
      }}>
        {t.hero.subtitle}
      </p>

      {/* Cost comparison */}
      <p style={{
        maxWidth: 600,
        margin: '0 auto 48px',
        fontFamily: 'var(--font-mono)',
        fontSize: 13,
        color: 'var(--ok)',
        opacity: 0.85,
      }}>
        {t.hero.costHint}
      </p>

      {/* CTA buttons */}
      <div style={{ display: 'flex', justifyContent: 'center', gap: 16, flexWrap: 'wrap' }}>
        <a
          href={BASE + 'docs/getting-started'}
          className="btn-primary"
          style={{ fontSize: 15, padding: '12px 28px' }}
        >
          {t.hero.getStarted} →
        </a>
        <a
          href="https://github.com/shannon-agent/shannon-code"
          target="_blank"
          rel="noopener noreferrer"
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            padding: '12px 28px',
            background: 'transparent',
            color: 'var(--muted)',
            border: '1px solid var(--line)',
            borderRadius: 'var(--radius-sm)',
            fontSize: 15,
            fontWeight: 500,
            textDecoration: 'none',
            transition: 'all 0.15s',
          }}
        >
          ⭐ {t.hero.starGithub} →
        </a>
      </div>
    </section>
  );
}
