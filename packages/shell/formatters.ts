/** Shared number formatters for shell widgets (CacheCliffWidget). */
export function formatTokens(num: number | null | undefined): string {
  if (num === null || num === undefined || Number.isNaN(num)) return '—';
  if (num >= 1_000_000_000) {
    return `${(num / 1_000_000_000).toFixed(2)}B`;
  }
  if (num >= 1_000_000) {
    return `${(num / 1_000_000).toFixed(1)}M`;
  }
  if (num >= 1_000) {
    return `${(num / 1_000).toFixed(1)}k`;
  }
  return num.toLocaleString();
}

export function formatUSD(amount: number | null | undefined): string {
  if (amount === null || amount === undefined || Number.isNaN(amount)) return '—';
  if (amount >= 1_000_000) {
    return `$${(amount / 1_000_000).toFixed(2)}M`;
  }
  if (amount >= 1_000) {
    return `$${(amount / 1_000).toFixed(2)}k`;
  }
  if (amount >= 100) {
    return `$${amount.toFixed(0)}`;
  }
  if (amount >= 1) {
    return `$${amount.toFixed(2)}`;
  }
  if (amount > 0) {
    return `$${amount.toFixed(2)}`;
  }
  return '$0.00';
}

/** Adapter display badge for fleet chips (and similar shell chrome). */
export function getAdapterBadge(adapter: string): {
  name: string;
  tag: string;
  borderColor: string;
} {
  switch (adapter) {
    case 'claude_code':
      return { name: 'Claude Code', tag: 'claude', borderColor: 'border-black' };
    case 'antigravity':
    case 'gemini':
      return { name: 'Antigravity (AGY)', tag: 'antigravity', borderColor: 'border-black' };
    case 'codex':
      return { name: 'Codex CLI', tag: 'codex', borderColor: 'border-black' };
    case 'cursor':
      return { name: 'Cursor Composer', tag: 'cursor', borderColor: 'border-black' };
    case 'goose':
      return { name: 'Block Goose', tag: 'goose', borderColor: 'border-black' };
    case 'pi':
      return { name: 'Pi Task Agent', tag: 'pi', borderColor: 'border-black' };
    case 'herdr':
      return { name: 'Herdr Swarm', tag: 'herdr', borderColor: 'border-black' };
    case 'hermes':
      return { name: 'Nous Hermes', tag: 'hermes', borderColor: 'border-black' };
    case 'openclaw':
      return { name: 'OpenClaw', tag: 'openclaw', borderColor: 'border-black' };
    case 'grok':
      return { name: 'xAI Grok', tag: 'grok', borderColor: 'border-black' };
    case 'opencode':
      return { name: 'OpenCode', tag: 'opencode', borderColor: 'border-black' };
    default:
      return { name: adapter, tag: adapter, borderColor: 'border-black' };
  }
}
