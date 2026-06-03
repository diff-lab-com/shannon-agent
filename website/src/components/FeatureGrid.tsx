import { useEffect, useRef, useState } from 'react';
import { type Lang, getTranslations } from '../i18n';

interface FeatureGridProps {
  lang: Lang;
}

export default function FeatureGrid({ lang }: FeatureGridProps) {
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
    <section ref={ref} id="features" style={{
      maxWidth: 'var(--max-width)',
      margin: '0 auto',
      padding: '80px 24px',
    }}>
      <h2 style={{ textAlign: 'center', marginBottom: 56 }}>{t.features.title}</h2>
      <div style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))',
        gap: 20,
      }}>
        {t.features.items.map((item, i) => (
          <div
            key={i}
            className="reveal"
            style={{
              background: 'var(--paper)',
              border: '1px solid var(--line)',
              borderRadius: 'var(--radius-md)',
              padding: '28px 24px',
              opacity: visible ? 1 : 0,
              transform: visible ? 'translateY(0)' : 'translateY(14px)',
              transition: `opacity 0.5s ease ${i * 0.08}s, transform 0.5s ease ${i * 0.08}s`,
            }}
          >
            <span style={{
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
              color: 'var(--accent)',
              fontWeight: 600,
              letterSpacing: '0.05em',
            }}>{item.num}</span>
            <h3 style={{ margin: '8px 0 10px', fontSize: 18 }}>{item.title}</h3>
            <p style={{ color: 'var(--muted)', fontSize: 14, lineHeight: 1.6, margin: 0 }}>{item.desc}</p>
          </div>
        ))}
      </div>
    </section>
  );
}
