import { useEffect, useRef, useState } from 'react';
import { type Lang, getTranslations } from '../i18n';

interface TrustSectionProps {
  lang: Lang;
}

export default function TrustSection({ lang }: TrustSectionProps) {
  const t = getTranslations(lang);
  const ref = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const observer = new IntersectionObserver(
      ([entry]) => { if (entry.isIntersecting) setVisible(true); },
      { threshold: 0.08 }
    );
    if (ref.current) observer.observe(ref.current);
    return () => observer.disconnect();
  }, []);

  return (
    <section ref={ref} id="security" style={{
      maxWidth: 'var(--max-width)',
      margin: '0 auto',
      padding: '80px 24px 40px',
    }}>
      <h2 style={{ textAlign: 'center', marginBottom: 48 }}>{t.trust.title}</h2>
      <div style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))',
        gap: 20,
      }}>
        {t.trust.cards.map((card, i) => (
          <div
            key={i}
            className="reveal"
            style={{
              background: 'var(--paper)',
              border: '1px solid var(--line)',
              borderRadius: 'var(--radius-md)',
              padding: '32px 28px',
              opacity: visible ? 1 : 0,
              transform: visible ? 'translateY(0)' : 'translateY(14px)',
              transition: `opacity 0.5s ease ${i * 0.1}s, transform 0.5s ease ${i * 0.1}s`,
            }}
          >
            <h3 style={{ margin: '0 0 8px', fontSize: 20 }}>{card.title}</h3>
            <p style={{ color: 'var(--muted)', fontSize: 14, lineHeight: 1.6, margin: '0 0 18px' }}>{card.desc}</p>
            <ul style={{ margin: 0, padding: 0, listStyle: 'none' }}>
              {card.bullets.map((bullet, j) => (
                <li key={j} style={{
                  display: 'flex',
                  gap: 10,
                  alignItems: 'flex-start',
                  padding: '7px 0',
                  color: 'var(--muted)',
                  fontSize: 14,
                  lineHeight: 1.55,
                  borderTop: j === 0 ? 'none' : '1px solid var(--line)',
                }}>
                  <span style={{ color: 'var(--ok)', fontFamily: 'var(--font-mono)', flexShrink: 0 }}>✓</span>
                  <span>{bullet}</span>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
    </section>
  );
}
