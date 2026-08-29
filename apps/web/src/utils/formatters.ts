export function formatTokens(num: number): string {
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
        className: 'bg-green-100 text-green-800 border-green-700 font-bold',
        symbol: '✓✓',
      };
    case 'commit_observed':
      return {
        label: 'COMMIT OBSERVED',
        className: 'bg-emerald-50 text-emerald-800 border-emerald-600',
        symbol: '✓ git',
      };
    case 'test_or_build_passed':
      return {
        label: 'TEST/BUILD PASSED',
        className: 'bg-green-50 text-green-700 border-green-500',
        symbol: '✓ pass',
      };
    case 'artifact_changed':
      return {
        label: 'ARTIFACT CHANGED',
        className: 'bg-yellow-50 text-amber-800 border-amber-500',
        symbol: '● diff',
      };
    case 'done_claimed':
      return {
        label: 'DONE CLAIMED',
        className: 'bg-gray-100 text-gray-700 border-gray-400',
        symbol: '? claim',
      };
    case 'unresolved':
    default:
      return {
        label: 'UNRESOLVED',
        className: 'bg-red-50 text-red-700 border-red-400',
        symbol: '✗ fail',
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
      return { name: 'Claude Code', tag: 'claude', borderColor: 'border-orange-600' };
    case 'codex':
      return { name: 'Codex CLI', tag: 'codex', borderColor: 'border-emerald-700' };
    case 'gemini':
      return { name: 'Gemini CLI', tag: 'gemini', borderColor: 'border-blue-600' };
    case 'opencode':
      return { name: 'OpenCode', tag: 'opencode', borderColor: 'border-purple-600' };
    default:
      return { name: adapter, tag: adapter, borderColor: 'border-zinc-600' };
  }
}
