import React from 'react';

export interface AgentLogoProps {
  className?: string;
  size?: number;
}

// 1. Anthropic Claude Code Logo (Official 14-spoke Terracotta Spark)
export const ClaudeLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="#D97757" className={className}>
    <path d="M4.5 16.5L8.9 13.7L9 13.4L8.9 13.2L8.7 13.2L7.9 13.1L5.4 13L3.2 12.9L1 12.8L0.5 12.6L0 12L0 11.6L0.5 11.3L1.1 11.3L2.6 11.4L4.8 11.6L6.3 11.7L8.6 12L9 12L9 11.8L8.9 11.7L8.8 11.6L6.6 10L4.2 8.3L2.9 7.3L2.2 6.8L1.9 6.3L1.7 5.3L2.3 4.6L3.2 4.7L3.4 4.8L4.3 5.4L6.1 6.9L8.4 8.7L8.8 9L8.9 8.9L8.9 8.8L8.8 8.6L7.5 6.1L6.1 3.6L5.5 2.6L5.3 2L5.2 1.3L6 0.3L6.4 0.1L7.4 0.3L7.8 0.6L8.4 2L9.4 4.3L10.8 7.3L11.3 8.2L11.5 9L11.6 9.3L11.7 9.3L11.7 9.1L11.8 7.4L12.1 5.3L12.3 2.6L12.4 1.9L12.7 1L13.5 0.5L14.1 0.8L14.5 1.5L14.5 1.9L14.2 3.7L13.7 6.6L13.3 8.6L13.5 8.6L13.8 8.3L14.7 7L16.4 5L17.1 4.1L17.9 3.2L18.5 2.8L19.5 2.8L20.2 3.9L19.9 5.1L18.8 6.4L17.9 7.6L16.7 9.3L15.9 10.6L16 10.7L16.2 10.7L18.9 10.1L20.4 9.8L22.2 9.5L23 9.9L23.1 10.3L22.8 11.1L20.8 11.6L18.6 12L15.2 12.8L15.1 12.9L15.2 12.9L16.7 13.1L17.3 13.1L18.9 13.1L21.9 13.4L22.7 13.9L23.1 14.5L23 15L21.8 15.6L20.2 15.2L16.5 14.3L15.2 14L15.1 14L15.1 14.1L16.1 15.2L18.1 17L20.5 19.3L20.6 19.8L20.3 20.3L20 20.2L17.8 18.6L17 17.8L15.1 16.2L15 16.2L15 16.4L15.4 17L17.7 20.6L17.8 21.6L17.6 22L17 22.2L16.4 22.1L15 20.1L13.7 18L12.6 16.1L12.5 16.1L11.8 23.4L11.5 23.7L10.8 24L10.2 23.5L9.9 22.8L10.2 21.3L10.6 19.4L10.9 17.9L11.2 16L11.3 15.4L11.3 15.3L11.2 15.4L9.8 17.3L7.7 20.2L6 22.1L5.6 22.2L4.9 21.9L4.9 21.2L5.3 20.6L7.7 17.6L9.1 15.7L10 14.7L10 14.5L9.9 14.5L4.1 18.6L3 18.7L2.5 18.3L2.6 17.5L2.8 17.3L4.5 16.5Z" />
  </svg>
);

// 2. Cursor Composer Logo (Official LobeHub / Anysphere Vector)
export const CursorLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" className={className}>
    <path
      d="M22.106 5.68L12.5.135a.998.998 0 00-.998 0L1.893 5.68a.84.84 0 00-.419.726v11.186c0 .3.16.577.42.727l9.607 5.547a.999.999 0 00.998 0l9.608-5.547a.84.84 0 00.42-.727V6.407a.84.84 0 00-.42-.726zm-.603 1.176L12.228 22.92c-.063.108-.228.064-.228-.061V12.34a.59.59 0 00-.295-.51l-9.11-5.26c-.107-.062-.063-.228.062-.228h18.55c.264 0 .428.286.296.514z"
      fill="#000000"
    />
  </svg>
);

// 3. Google Gemini / Antigravity Logo (Official 4-point Sparkle Star)
export const GeminiLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" className={className}>
    <path
      d="M12 2C12 7.5 7.5 12 2 12C7.5 12 12 16.5 12 22C12 16.5 16.5 12 22 12C16.5 12 12 7.5 12 2Z"
      fill="url(#gemini-grad)"
    />
    <defs>
      <linearGradient id="gemini-grad" x1="2" y1="2" x2="22" y2="22" gradientUnits="userSpaceOnUse">
        <stop stopColor="#1A73E8" />
        <stop offset="1" stopColor="#8AB4F8" />
      </linearGradient>
    </defs>
  </svg>
);

// 4. OpenAI Codex Logo (Official Swirl)
export const CodexLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="#10A37F" className={className}>
    <path d="M22.28 9.82a6 6 0 0 0-.51-4.91 6.05 6.05 0 0 0-6.52-2.9A6.06 6.06 0 0 0 4.98 4.18a6 6 0 0 0-4 2.9 6.05 6.05 0 0 0 .74 7.1 5.98 5.98 0 0 0 .51 4.91 6.05 6.05 0 0 0 6.52 2.9A6 6 0 0 0 13.26 24a6.06 6.06 0 0 0 5.77-4.2 6 6 0 0 0 4-2.9 6.06 6.06 0 0 0-.75-7.08zM13.26 22.43a4.48 4.48 0 0 1-2.88-1.04l.14-.08 4.78-2.76a.8.8 0 0 0 .4-.68v-6.74l2.01 1.17a.07.07 0 0 1 .04.05v5.58a4.5 4.5 0 0 1-4.49 4.5zm-9.66-4.13a4.47 4.47 0 0 1-.54-3.01l.14.08 4.79 2.76a.77.77 0 0 0 .78 0l5.84-3.37v2.33a.08.08 0 0 1-.03.06L9.74 19.95a4.5 4.5 0 0 1-6.14-1.65zM2.34 7.9a4.5 4.5 0 0 1 2.37-1.98V11.6a.77.77 0 0 0 .38.68l5.82 3.35-2.02 1.17a.08.08 0 0 1-.07 0l-4.83-2.79A4.5 4.5 0 0 1 2.34 7.87zm16.6 3.85l-5.84-3.38L15.12 7.2a.08.08 0 0 1 .07 0l4.83 2.79a4.5 4.5 0 0 1-.68 8.1v-5.67a.79.79 0 0 0-.4-.67zm2.01-3.02l-.14-.09-4.77-2.78a.78.78 0 0 0-.79 0L9.41 9.23V6.9a.07.07 0 0 1 .03-.06l4.83-2.79a4.5 4.5 0 0 1 6.68 4.66zm-12.64 4.13l-2.02-1.16a.08.08 0 0 1-.04-.06V6.08a4.5 4.5 0 0 1 7.38-3.46l-.14.08-4.79 2.76a.8.8 0 0 0-.39.68zm1.1-2.36l2.6-1.5 2.6 1.5v3l-2.6 1.5-2.6-1.5z" />
  </svg>
);

// 5. Block Goose Logo (Official Block / Goose Vector)
export const GooseLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" className={className}>
    <path
      d="M21.595 23.61c1.167-.254 2.405-.944 2.405-.944l-2.167-1.784a12.124 12.124 0 01-2.695-3.131 12.127 12.127 0 00-3.97-4.049l-.794-.462a1.115 1.115 0 01-.488-.815.844.844 0 01.154-.575c.413-.582 2.548-3.115 2.94-3.44.503-.416 1.065-.762 1.586-1.159.074-.056.148-.112.221-.17.003-.002.007-.004.009-.007.167-.131.325-.272.45-.438.453-.524.563-.988.59-1.193-.061-.197-.244-.639-.753-1.148.319.02.705.272 1.056.569.235-.376.481-.773.727-1.171.165-.266-.08-.465-.086-.471h-.001V3.22c-.007-.007-.206-.25-.471-.086-.567.35-1.134.702-1.639 1.021 0 0-.597-.012-1.305.599a2.464 2.464 0 00-.438.45l-.007.009c-.058.072-.114.147-.17.221-.397.521-.743 1.083-1.16 1.587-.323.391-2.857 2.526-3.44 2.94a.842.842 0 01-.574.153 1.115 1.115 0 01-.815-.488l-.462-.794a12.123 12.123 0 00-4.049-3.97 12.133 12.133 0 01-3.13-2.695L1.332 0S.643 1.238.39 2.405c.352.428 1.27 1.49 2.34 2.302C1.58 4.167.73 3.75.06 3.4c-.103.765-.063 1.92.043 2.816.726.317 1.961.806 3.219 1.066-1.006.236-2.11.278-2.961.262.15.554.358 1.119.64 1.688.119.263.25.52.39.77.452.125 2.222.383 3.164.171l-2.51.897a27.776 27.776 0 002.544 2.726c2.031-1.092 2.494-1.241 4.018-2.238-2.467 2.008-3.108 2.828-3.8 3.67l-.483.678c-.25.351-.469.725-.65 1.117-.61 1.31-1.47 4.1-1.47 4.1-.154.486.202.842.674.674 0 0 2.79-.861 4.1-1.47.392-.182.766-.4 1.118-.65l.677-.483c.227-.187.453-.37.701-.586 0 0 1.705 2.02 3.458 3.349l.896-2.511c-.211.942.046 2.712.17 3.163.252.142.509.272.772.392.569.28 1.134.49 1.688.64-.016-.853.026-1.956.261-2.962.26 1.258.75 2.493 1.067 3.219.895.106 2.051.146 2.816.043a73.87 73.87 0 01-1.308-2.67c.811 1.07 1.874 1.988 2.302 2.34h-.001z"
      fill="#E54D2E"
    />
  </svg>
);

// 6. OpenCode Logo (Official Bracket Matrix)
export const OpenCodeLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" className={className}>
    <rect x="2" y="4" width="9" height="7" rx="1" fill="#111111" />
    <rect x="13" y="4" width="9" height="7" rx="1" fill="#777777" />
    <rect x="2" y="13" width="9" height="7" rx="1" fill="#777777" />
    <rect x="13" y="13" width="9" height="7" rx="1" fill="#111111" />
  </svg>
);

// 7. Nous Hermes Logo (Winged Helm)
export const HermesLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" className={className}>
    <path d="M4 16c2-4 6-6 8-6s6 2 8 6" stroke="#7950F2" strokeWidth="2" strokeLinecap="round" />
    <path d="M12 10V4M7 6l5-2 5 2" stroke="#7950F2" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    <circle cx="12" cy="16" r="2" fill="#7950F2" />
  </svg>
);

// 8. xAI Grok Logo (Official Geometric Slash)
export const GrokLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0.36 0.5 33.33 32" fill="none" className={className}>
    <path d="M13.24 21.04L24.32 12.85c.54-.4 1.32-.24 1.58.38 1.36 3.29.75 7.24-1.96 9.96-2.71 2.71-6.48 3.31-9.93 1.95L10.24 26.88c5.4 3.7 11.96 2.78 16.06-1.32 3.25-3.26 4.26-7.7 3.32-11.7l.01.01C28.26 7.87 29.96 5.52 33.45.72l.25-.22-4.59 4.59V5.08L13.23 21.04z" fill="#000" />
    <path d="M10.95 23.03C7.07 19.32 7.74 13.59 11.05 10.28c2.45-2.45 6.45-3.45 9.95-1.98L24.76 6.56c-.68-.49-1.55-1.02-2.54-1.39-4.5-1.85-9.89-.93-13.55 2.73-3.52 3.52-4.62 8.94-2.72 13.56 1.42 3.45-.9 5.9-3.25 8.36-.83.88-1.66 1.75-2.34 2.68L10.95 23.03z" fill="#000" />
  </svg>
);

// 9. Pi Logo (Greek Pi)
export const PiLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" className={className}>
    <path d="M3 6h18M8 6v12M16 6v11c0 1.5 1 1.5 2 1.5" stroke="#0066CC" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

// 10. OpenClaw Logo (Mechanical Claw)
export const OpenClawLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" className={className}>
    <path d="M6 5l6 6 6-6M5 13l7 7 7-7" stroke="#EA580C" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

// 11. Herdr Logo (Multi-Agent Swarm DAG)
export const HerdrLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" className={className}>
    <circle cx="6" cy="12" r="3" fill="#111111" />
    <circle cx="18" cy="6" r="3" fill="#111111" />
    <circle cx="18" cy="18" r="3" fill="#111111" />
    <path d="M9 11l6-3.5M9 13l6 3.5" stroke="#111111" strokeWidth="1.8" strokeLinecap="round" />
  </svg>
);

export const AgentLogoIcon: React.FC<{ adapterId: string; size?: number; className?: string }> = ({
  adapterId,
  size = 20,
  className = '',
}) => {
  switch (adapterId) {
    case 'claude_code':
      return <ClaudeLogo size={size} className={className} />;
    case 'cursor':
      return <CursorLogo size={size} className={className} />;
    case 'antigravity':
      return <GeminiLogo size={size} className={className} />;
    case 'codex':
      return <CodexLogo size={size} className={className} />;
    case 'goose':
      return <GooseLogo size={size} className={className} />;
    case 'opencode':
      return <OpenCodeLogo size={size} className={className} />;
    case 'hermes':
      return <HermesLogo size={size} className={className} />;
    case 'grok':
      return <GrokLogo size={size} className={className} />;
    case 'pi':
      return <PiLogo size={size} className={className} />;
    case 'openclaw':
      return <OpenClawLogo size={size} className={className} />;
    case 'herdr':
      return <HerdrLogo size={size} className={className} />;
    default:
      return null;
  }
};

export const getAgentLogo = (adapterId: string, size = 20, className = '') => (
  <AgentLogoIcon adapterId={adapterId} size={size} className={className} />
);

