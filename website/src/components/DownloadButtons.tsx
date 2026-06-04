import { useState, useEffect } from 'react';
import { type Lang, getTranslations } from '../i18n';

type OS = 'macos' | 'linux' | 'windows';
type Arch = 'arm64' | 'x64';

interface DownloadButtonsProps {
  lang: Lang;
}

const GH_RELEASE = 'https://github.com/shannon-agent/shannon-code/releases/latest/download';
const CDN_BASE = import.meta.env.PUBLIC_CDN_BASE || GH_RELEASE;

function detectOS(): OS {
  if (typeof navigator === 'undefined') return 'linux';
  const ua = navigator.userAgent;
  if (ua.includes('Mac')) return 'macos';
  if (ua.includes('Win')) return 'windows';
  return 'linux';
}

function detectArch(): Arch {
  if (typeof navigator === 'undefined') return 'x64';
  const ua = navigator.userAgent;
  if (ua.includes('arm64') || ua.includes('aarch64')) return 'arm64';
  return 'x64';
}

const DOWNLOADS: Record<OS, Record<Arch, { url: string; label: string }>> = {
  macos: {
    arm64: { url: `${CDN_BASE}/shannon-cli-aarch64-apple-darwin.tar.gz`, label: 'macOS (Apple Silicon)' },
    x64: { url: `${CDN_BASE}/shannon-cli-x86_64-apple-darwin.tar.gz`, label: 'macOS (Intel)' },
  },
  linux: {
    arm64: { url: `${CDN_BASE}/shannon-cli-aarch64-unknown-linux-musl.tar.gz`, label: 'Linux (ARM64)' },
    x64: { url: `${CDN_BASE}/shannon-cli-x86_64-unknown-linux-musl.tar.gz`, label: 'Linux (x86_64)' },
  },
  windows: {
    arm64: { url: `${CDN_BASE}/shannon-cli-x86_64-pc-windows-msvc.zip`, label: 'Windows (x86_64)' },
    x64: { url: `${CDN_BASE}/shannon-cli-x86_64-pc-windows-msvc.zip`, label: 'Windows (x86_64)' },
  },
};

const OS_LABELS: Record<OS, Record<Lang, string>> = {
  macos: { en: 'macOS', zh: 'macOS' },
  linux: { en: 'Linux', zh: 'Linux' },
  windows: { en: 'Windows', zh: 'Windows' },
};

const INSTALL_COMMANDS: Record<OS, Record<Lang, string>> = {
  macos: {
    en: `curl -fsSL ${CDN_BASE}/install.sh | sh`,
    zh: `curl -fsSL ${CDN_BASE}/install.sh | sh`,
  },
  linux: {
    en: `curl -fsSL ${CDN_BASE}/install.sh | sh`,
    zh: `curl -fsSL ${CDN_BASE}/install.sh | sh`,
  },
  windows: {
    en: `irm ${CDN_BASE}/install.ps1 | iex`,
    zh: `irm ${CDN_BASE}/install.ps1 | iex`,
  },
};

export default function DownloadButtons({ lang }: DownloadButtonsProps) {
  const t = getTranslations(lang);
  const [detectedOS, setDetectedOS] = useState<OS>('linux');
  const [detectedArch, setDetectedArch] = useState<Arch>('x64');
  const [selectedOS, setSelectedOS] = useState<OS>('linux');
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const os = detectOS();
    const arch = detectArch();
    setDetectedOS(os);
    setDetectedArch(arch);
    setSelectedOS(os);
  }, []);

  const download = DOWNLOADS[selectedOS][detectedArch];
  const allOSes: OS[] = ['macos', 'linux', 'windows'];
  const copyCommand = () => {
    navigator.clipboard.writeText(INSTALL_COMMANDS[selectedOS][lang]);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  const isDetected = selectedOS === detectedOS;

  return (
    <div>
      {/* OS tabs */}
      <div style={{ display: 'flex', gap: 8, marginBottom: 20 }}>
        {allOSes.map(os => (
          <button
            key={os}
            onClick={() => setSelectedOS(os)}
            style={{
              padding: '10px 20px',
              background: selectedOS === os ? 'var(--accent)' : 'var(--paper)',
              color: selectedOS === os ? 'white' : 'var(--body)',
              border: `1px solid ${selectedOS === os ? 'var(--accent)' : 'var(--line)'}`,
              borderRadius: 'var(--radius-sm)',
              fontFamily: 'var(--font-mono)',
              fontSize: 13,
              fontWeight: 500,
              cursor: 'pointer',
              transition: 'all 0.15s',
              position: 'relative',
            }}
          >
            {OS_LABELS[os][lang]}
            {isDetected && os === detectedOS && (
              <span style={{
                position: 'absolute', top: -6, right: -6,
                background: 'var(--ok)', color: 'white',
                fontSize: 9, padding: '1px 5px',
                borderRadius: 'var(--radius-pill)',
                fontWeight: 600,
              }}>
                {lang === 'en' ? 'detected' : '已检测'}
              </span>
            )}
          </button>
        ))}
      </div>

      {/* Install command */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        background: 'var(--code-bg)',
        borderRadius: 'var(--radius-sm)',
        padding: '14px 18px',
        marginBottom: 16,
      }}>
        <code style={{
          flex: 1,
          fontFamily: 'var(--font-mono)',
          fontSize: 13,
          color: 'var(--code-text)',
          overflowX: 'auto',
          whiteSpace: 'nowrap',
        }}>
          {INSTALL_COMMANDS[selectedOS][lang]}
        </code>
        <button
          onClick={copyCommand}
          style={{
            background: copied ? 'rgba(52, 211, 153, 0.15)' : 'rgba(255,255,255,0.06)',
            border: `1px solid ${copied ? 'var(--ok)' : 'rgba(255,255,255,0.12)'}`,
            borderRadius: 'var(--radius-sm)',
            padding: '5px 12px',
            fontFamily: 'var(--font-mono)',
            fontSize: 12,
            color: copied ? 'var(--ok)' : 'var(--muted)',
            cursor: 'pointer',
            transition: 'all 0.15s',
            whiteSpace: 'nowrap',
          }}
        >
          {copied ? (lang === 'en' ? 'Copied!' : '已复制！') : 'Copy'}
        </button>
      </div>

      {/* Direct download */}
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        <a
          href={download.url}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 8,
            padding: '10px 22px',
            background: 'var(--accent)',
            color: 'white',
            border: 'none',
            borderRadius: 'var(--radius-sm)',
            fontSize: 14,
            fontWeight: 500,
            textDecoration: 'none',
            transition: 'opacity 0.15s',
          }}
        >
          {lang === 'en' ? `Download for ${download.label}` : `下载 ${download.label}`}
        </a>
        {selectedOS === 'macos' && (
          <a
            href="#"
            onClick={e => { e.preventDefault(); }}
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: 6,
              padding: '10px 18px',
              background: 'var(--paper)',
              border: '1px solid var(--line)',
              borderRadius: 'var(--radius-sm)',
              fontSize: 13,
              color: 'var(--body)',
              textDecoration: 'none',
            }}
          >
            brew install shannon-agent/tap/shannon
          </a>
        )}
      </div>
    </div>
  );
}
