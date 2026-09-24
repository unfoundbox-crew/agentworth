/**
 * Token counts, defensively.
 *
 * These formatters are fed straight from API responses, and this repo's API
 * has drifted from its TypeScript types repeatedly — field renames, fields
 * moving under a parent object, fields disappearing between releases. An
 * undefined reaching `num.toLocaleString()` throws, and because these render
 * inside the inspector it takes the whole pane down behind an error boundary
 * rather than showing one blank number. Observed against the 0.1.11 binary,
 * whose `/api/stats` moved `first_session_at` under `date_range`.
 *
 * A missing number renders as an em dash, which is the honest thing to show
 * for a value the server did not send.
 */
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

export function formatDuration(seconds?: number): string {
  if (!seconds || seconds <= 0) return '0s';
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  const h = Math.floor(m / 60);
  if (h > 0) {
    return `${h}h ${m % 60}m`;
  }
  if (m > 0) {
    return `${m}m ${s}s`;
  }
  return `${s}s`;
}

export function formatDate(isoStr: string): string {
  try {
    const d = new Date(isoStr);
    return d.toLocaleString('en-US', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    });
  } catch {
    return isoStr;
  }
}

export function formatTimeAgo(isoStr: string): string {
  try {
    const d = new Date(isoStr);
    const now = new Date('2026-08-29T14:10:00Z');
    const diffSec = Math.floor((now.getTime() - d.getTime()) / 1000);
    if (diffSec < 60) return `${diffSec}s ago`;
    const diffMin = Math.floor(diffSec / 60);
    if (diffMin < 60) return `${diffMin}m ago`;
    const diffHours = Math.floor(diffMin / 60);
    if (diffHours < 24) return `${diffHours}h ago`;
    const diffDays = Math.floor(diffHours / 24);
    return `${diffDays}d ago`;
  } catch {
    return isoStr;
  }
}

export function getOutcomeBadgeInfo(outcome?: string): {
  label: string;
  className: string;
  symbol: string;
} {
  switch (outcome) {
    case 'ci_or_deployment_verified':
      return {
        label: 'CI/PR VERIFIED',
        className: 'bg-black text-white border-black font-bold',
        symbol: '✓✓',
      };
    case 'commit_observed':
      return {
        label: 'COMMIT OBSERVED',
        className: 'bg-zinc-900 text-zinc-100 border-zinc-900 font-medium',
        symbol: '✓ git',
      };
    case 'test_or_build_passed':
      return {
        label: 'TESTS PASSED',
        className: 'bg-zinc-800 text-zinc-100 border-zinc-800 font-medium',
        symbol: '✓ pass',
      };
    case 'artifact_changed':
      return {
        label: 'ARTIFACT CHANGED',
        className: 'bg-zinc-200 text-zinc-900 border-zinc-400 font-medium',
        symbol: '● diff',
      };
    case 'done_claimed':
      return {
        label: 'DONE CLAIMED',
        className: 'bg-zinc-100 text-zinc-700 border-zinc-300',
        symbol: '? claim',
      };
    case 'unresolved':
    default:
      return {
        label: 'UNRESOLVED',
        className: 'bg-white text-zinc-900 border-dashed border-zinc-400',
        symbol: '✗ unres',
      };
  }
}

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

/**
 * Estimates developer token expenditure in USD based on total tokens and model breakdown or typical pricing.
 * Default blended baseline is ~$3.00 per million tokens.
 */
export function estimateTokenCostUSD(
  tokens: number,
  models?: string[] | Record<string, number>
): number {
  if (!tokens || tokens <= 0) return 0;

  if (models && typeof models === 'object' && !Array.isArray(models)) {
    let totalEstimated = 0;
    let totalCount = 0;
    for (const [modelName, count] of Object.entries(models)) {
      const rate = getModelBlendedRatePerMillion(modelName);
      totalEstimated += count * rate;
      totalCount += count;
    }
    if (totalCount > 0) {
      const avgRate = totalEstimated / totalCount;
      return (tokens / 1_000_000) * avgRate;
    }
  }

  if (Array.isArray(models) && models.length > 0) {
    const totalRate = models.reduce((acc, m) => acc + getModelBlendedRatePerMillion(m), 0);
    const avgRate = totalRate / models.length;
    return (tokens / 1_000_000) * avgRate;
  }

  return (tokens / 1_000_000) * 3.0;
}

function getModelBlendedRatePerMillion(model: string): number {
  const m = model.toLowerCase();
  if (m.includes('opus')) return 15.0; // Claude Opus
  if (m.includes('sonnet')) return 3.0; // Claude Sonnet
  if (m.includes('haiku')) return 0.50; // Claude Haiku
  if (m.includes('o1') || m.includes('o3')) return 20.0;
  if (m.includes('gpt-4o-mini')) return 0.30;
  if (m.includes('gpt-4o') || m.includes('gpt-4')) return 3.5;
  if (m.includes('gemini-2.5-pro') || m.includes('gemini-1.5-pro')) return 2.0;
  if (m.includes('flash')) return 0.20;
  if (m.includes('deepseek')) return 0.35;
  if (m.includes('qwen') || m.includes('llama') || m.includes('mistral')) return 0.40;
  return 3.0; // blended default: ~$3.00 / 1M tokens
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

