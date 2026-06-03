import { useEffect, useRef, useState } from 'react';
import { type Lang, getTranslations } from '../i18n';

interface ComparisonTableProps {
  lang: Lang;
}

export default function ComparisonTable({ lang }: ComparisonTableProps) {
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
    <section ref={ref} style={{
      maxWidth: 'var(--max-width)',
      margin: '0 auto',
      padding: '40px 24px 80px',
    }}>
      <h2 style={{ textAlign: 'center', marginBottom: 40 }}>{t.comparison.title}</h2>

      {/* Stats row */}
      <div className="stats-grid" style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(4, 1fr)',
        gap: 16,
        marginBottom: 48,
      }}>
        {t.comparison.items.map((item, i) => (
          <div
            key={i}
            style={{
              textAlign: 'center',
              padding: '24px 12px',
              background: 'var(--paper)',
              border: '1px solid var(--line)',
              borderRadius: 'var(--radius-md)',
              opacity: visible ? 1 : 0,
              transform: visible ? 'translateY(0)' : 'translateY(10px)',
              transition: `opacity 0.4s ease ${i * 0.1}s, transform 0.4s ease ${i * 0.1}s`,
            }}
          >
            <div style={{
              fontFamily: 'var(--font-mono)',
              fontSize: 32,
              fontWeight: 700,
              color: 'var(--ink)',
              lineHeight: 1.2,
            }}>{item.value}</div>
            <div style={{ color: 'var(--muted)', fontSize: 14, marginTop: 4 }}>{item.label}</div>
          </div>
        ))}
      </div>

      {/* Comparison rows */}
      <div className="comparison-table-wrapper" style={{
        background: 'var(--paper)',
        borderRadius: 'var(--radius-lg)',
        border: '1px solid var(--line)',
        overflow: 'hidden',
      }}>
        <div style={{
          display: 'grid',
          gridTemplateColumns: '1fr 2fr 2fr',
          gap: 0,
          borderBottom: '1px solid var(--line)',
          padding: '12px 20px',
          background: 'var(--bg-soft)',
        }}>
          <span style={{ fontWeight: 600, fontSize: 13, color: 'var(--muted)' }} />
          <span style={{ fontWeight: 600, fontSize: 13, color: 'var(--accent)', textAlign: 'center' }}>Shannon Code</span>
          <span style={{ fontWeight: 600, fontSize: 13, color: 'var(--muted)', textAlign: 'center' }}>
            {lang === 'en' ? 'Typical Alternative' : '典型替代方案'}
          </span>
        </div>
        {t.comparison.rows.map((row, i) => (
          <div key={i} style={{
            display: 'grid',
            gridTemplateColumns: '1fr 2fr 2fr',
            gap: 0,
            padding: '16px 20px',
            borderBottom: i < t.comparison.rows.length - 1 ? '1px solid var(--line)' : 'none',
            alignItems: 'center',
          }}>
            <span style={{ fontWeight: 500, fontSize: 14, color: 'var(--ink)' }}>{row.feature}</span>
            <span style={{ color: 'var(--ok)', textAlign: 'center', fontFamily: 'var(--font-mono)', fontSize: 13 }}>{row.shannon}</span>
            <span style={{ fontSize: 13, color: 'var(--muted)', textAlign: 'center' }}>{row.other}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
