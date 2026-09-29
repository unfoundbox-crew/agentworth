import { TokenUsage } from '../types';
import { formatTokens } from '../utils/formatters';

// GET /api/traces/:id serializes TokenUsage with the Rust struct's field
// names (cache_read_tokens / cache_creation_tokens), not the
// cache_read_input_tokens / cache_creation_input_tokens the TS type
// declares. Verified against a live trace on 2026-09-01 — the API's own
// /api/stats normalization in services/api.ts already works around the
// same mismatch. Read both so the cache split (the headline economics
// story here) doesn't silently render as zero.
type RawTokenUsage = Partial<TokenUsage> & {
  cache_read_tokens?: number;
  cache_creation_tokens?: number;
};

export interface TokenFigures {
  input: number;
  output: number;
  cacheRead: number;
  cacheCreation: number;
  total: number;
}

export function readTokenUsage(usage: TokenUsage | null | undefined): TokenFigures {
  const raw = (usage ?? {}) as RawTokenUsage;
  const input = raw.input_tokens ?? 0;
  const output = raw.output_tokens ?? 0;
  const cacheRead = raw.cache_read_input_tokens ?? raw.cache_read_tokens ?? 0;
  const cacheCreation = raw.cache_creation_input_tokens ?? raw.cache_creation_tokens ?? 0;
  return { input, output, cacheRead, cacheCreation, total: input + output + cacheRead + cacheCreation };
}

interface Row {
  key: string;
  label: string;
  value: number;
  cls: string;
}

export interface TokenEconomicsProps {
  tokenUsage: TokenUsage;
}

/** What was spent (input/output) and what the cache did for you (read vs
 * creation) — the two stories behind a session's token exhaust. */
export function TokenEconomics({ tokenUsage }: TokenEconomicsProps) {
  const { input, output, cacheRead, cacheCreation, total } = readTokenUsage(tokenUsage);

  const rows: Row[] = [
    { key: 'input', label: 'Input', value: input, cls: 'is-input' },
    { key: 'output', label: 'Output', value: output, cls: 'is-output' },
    { key: 'cache_read', label: 'Cache read', value: cacheRead, cls: 'is-cache-read' },
    { key: 'cache_creation', label: 'Cache creation', value: cacheCreation, cls: 'is-cache-creation' },
  ];

  return (
    <div className="shell-tokens-block">
      <div className="shell-section-title">Token economics</div>

      {total > 0 && (
        <div className="shell-tokens-bar" role="img" aria-label="Input, output and cache token split">
          {rows.map((r) =>
            r.value > 0 ? (
              <div
                key={r.key}
                className={`shell-tokens-seg ${r.cls}`}
                style={{ width: `${(r.value / total) * 100}%` }}
                title={`${r.label}: ${r.value.toLocaleString()}`}
              />
            ) : null
          )}
        </div>
      )}

      <div className="shell-tokens-rows">
        {rows.map((r) => (
          <div className="shell-tokens-row" key={r.key}>
            <span className={`shell-tokens-swatch ${r.cls}`} aria-hidden="true" />
            <span className="shell-tokens-key">{r.label}</span>
            <span className="shell-tokens-val" title={r.value.toLocaleString()}>
              {formatTokens(r.value)}
            </span>
          </div>
        ))}
        <div className="shell-tokens-row shell-tokens-row-total">
          <span className="shell-tokens-swatch" aria-hidden="true" />
          <span className="shell-tokens-key">Total exhaust</span>
          <span className="shell-tokens-val" title={total.toLocaleString()}>
            {formatTokens(total)}
          </span>
        </div>
      </div>
    </div>
  );
}
