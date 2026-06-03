import { useState } from 'react';
import { type Lang, getTranslations } from '../i18n';

interface HeroProps {
  lang: Lang;
}

export default function Hero({ lang }: HeroProps) {
  const t = getTranslations(lang);
  const [activeTab, setActiveTab] = useState<'cargo' | 'curl' | 'brew'>('cargo');
  const [copied, setCopied] = useState(false);

  const copyCommand = () => {
    navigator.clipboard.writeText(t.hero.installCommands[activeTab]);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  const tabs = ['cargo', 'curl', 'brew'] as const;

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

      {/* Install panel */}
      <div id="install" style={{
        maxWidth: 620,
        margin: '0 auto 24px',
        background: 'var(--paper)',
        borderRadius: 'var(--radius-lg)',
        border: '1px solid var(--line)',
        overflow: 'hidden',
      }}>
        {/* Tabs */}
        <div style={{ display: 'flex', borderBottom: '1px solid var(--line)' }}>
          {tabs.map(tab => (
            <button
              key={tab}
              onClick={() => { setActiveTab(tab); setCopied(false); }}
              style={{
                flex: 1,
                padding: '12px 0',
                background: activeTab === tab ? 'var(--accent)' : 'transparent',
                color: activeTab === tab ? 'white' : 'var(--muted)',
                border: 'none',
                fontFamily: 'var(--font-mono)',
                fontSize: 13,
                fontWeight: 500,
                cursor: 'pointer',
                transition: 'background 0.15s, color 0.15s',
              }}
            >
              {t.hero.installTabs[tab]}
            </button>
          ))}
        </div>
        {/* Command */}
        <div style={{
          display: 'flex',
          alignItems: 'center',
          padding: '16px 20px',
          gap: 12,
        }}>
          <code style={{
            flex: 1,
            fontFamily: 'var(--font-mono)',
            fontSize: 14,
            color: 'var(--ink)',
            textAlign: 'left',
            overflowX: 'auto',
            whiteSpace: 'nowrap',
          }}>
            {t.hero.installCommands[activeTab]}
          </code>
          <button
            onClick={copyCommand}
            style={{
              background: copied ? 'rgba(52, 211, 153, 0.1)' : 'transparent',
              border: `1px solid ${copied ? 'var(--ok)' : 'var(--line)'}`,
              borderRadius: 'var(--radius-sm)',
              padding: '6px 14px',
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
              color: copied ? 'var(--ok)' : 'var(--muted)',
              cursor: 'pointer',
              transition: 'all 0.15s',
              whiteSpace: 'nowrap',
            }}
          >
            {copied ? t.hero.copied : t.hero.copy}
          </button>
        </div>
      </div>

      {/* Star link */}
      <a
        href="https://github.com/shannon-agent/shannon-code"
        target="_blank"
        rel="noopener noreferrer"
        style={{ color: 'var(--muted)', fontSize: 15, textDecoration: 'none' }}
      >
        ⭐ {t.hero.starGithub} →
      </a>
    </section>
  );
}
