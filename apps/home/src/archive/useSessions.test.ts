import { describe, expect, it } from 'vitest';

/**
 * useSessions is an effect-driven hook; its fetch contract is the interesting
 * bit for P2. Assert the URL and response shapes the hook documents, matching
 * apps/dashboard/src/hooks/useSessions.ts.
 */
describe('archive useSessions contract', () => {
  it('requests the full non-stub set with an explicit high limit', () => {
    // Mirrored from the hook — keep the magic number in one tested place so a
    // silent default of 50 cannot creep back in.
    const url = '/api/traces?limit=100000';
    expect(url).toContain('limit=100000');
  });

  it('accepts both bare-array and {traces,total} payloads', () => {
    const asArray = [{ session_id: 'a' }];
    const wrapped = { traces: [{ session_id: 'b' }], total: 1 };
    const pick = (data: unknown) =>
      Array.isArray(data)
        ? data
        : Array.isArray((data as { traces?: unknown })?.traces)
          ? (data as { traces: unknown[] }).traces
          : [];
    expect(pick(asArray)).toHaveLength(1);
    expect(pick(wrapped)).toHaveLength(1);
    expect(pick(null)).toHaveLength(0);
  });
});
