import { useEffect, useState, useRef } from 'react';
import { type Lang, getTranslations } from '../i18n';

interface TerminalProps {
  lang: Lang;
}

interface TermLine {
  type: 'prompt' | 'output' | 'tool';
  text: string;
}

export default function Terminal({ lang }: TerminalProps) {
  const t = getTranslations(lang);
  const [visibleLines, setVisibleLines] = useState(0);
  const [started, setStarted] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  const lines: TermLine[] = t.terminal.lines as unknown as TermLine[];
  const delays = [300, 450, 300, 600, 450, 350, 500, 400, 400];

  useEffect(() => {
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting && !started) {
          setStarted(true);
        }
      },
      { threshold: 0.12 }
    );
    if (ref.current) observer.observe(ref.current);
    return () => observer.disconnect();
  }, [started]);

  useEffect(() => {
    if (!started) return;
    if (visibleLines >= lines.length) return;
    const delay = delays[visibleLines] || 400;
    const timer = setTimeout(() => setVisibleLines(v => v + 1), delay);
    return () => clearTimeout(timer);
  }, [started, visibleLines, lines.length]);

  const getLineColor = (type: string) => {
    switch (type) {
      case 'prompt': return '#7aa2ff';
      case 'tool': return '#c7a3f0';
      default: return '#cdd6f4';
    }
  };

  return (
    <div ref={ref} style={{
      maxWidth: 740,
      margin: '40px auto 80px',
      background: 'var(--code-bg)',
      borderRadius: 'var(--radius-lg)',
      boxShadow: 'var(--shadow-terminal)',
      overflow: 'hidden',
    }}>
      {/* macOS dots */}
      <div style={{ display: 'flex', gap: 8, padding: '14px 18px' }}>
        <span style={{ width: 12, height: 12, borderRadius: '50%', background: '#ff5f57' }} />
        <span style={{ width: 12, height: 12, borderRadius: '50%', background: '#febc2e' }} />
        <span style={{ width: 12, height: 12, borderRadius: '50%', background: '#28c840' }} />
      </div>
      {/* Terminal lines */}
      <div style={{ padding: '4px 20px 20px', fontFamily: 'var(--font-mono)', fontSize: 14 }}>
        {lines.map((line, i) => (
          <div
            key={i}
            style={{
              color: getLineColor(line.type),
              padding: '3px 0',
              opacity: i < visibleLines ? 1 : 0,
              transform: i < visibleLines ? 'translateY(0)' : 'translateY(3px)',
              transition: 'opacity 0.3s ease, transform 0.3s ease',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-all',
            }}
          >
            {line.text}
          </div>
        ))}
        {/* Blinking cursor */}
        {visibleLines >= lines.length && (
          <span style={{
            display: 'inline-block',
            width: 7,
            height: 15,
            background: '#7aa2ff',
            animation: 'blink 1s steps(1) infinite',
            verticalAlign: 'middle',
          }} />
        )}
      </div>
      <style>{`@keyframes blink { 0%, 100% { opacity: 1 } 50% { opacity: 0 } }`}</style>
    </div>
  );
}
