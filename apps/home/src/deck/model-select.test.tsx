import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import type { Harness } from '../protocol';
import { STATIC_CATALOG, filterModels, groupModels, shouldShowVariantRow, visibleModels, type CatalogModel } from '../model/catalog';
import {
  dialogReducer,
  loadSelection,
  saveSelection,
  quotaDialogContent,
  type ModelSelection,
} from '../model/modelSelection';
import { ModelDialog } from './ModelDialog';
import { QuotaDialog } from './ModelDialog';
import { FirstDirection } from './FirstDirection';

const HARNESSES: Harness[] = [
  { id: 'claude', label: 'Claude', bin: 'claude' },
  { id: 'codex', label: 'Codex', bin: 'codex' },
  { id: 'agy', label: 'Antigravity', bin: 'agy' },
  { id: 'opencode', label: 'opencode', bin: 'opencode' },
  { id: 'cursor', label: 'Cursor', bin: 'cursor-agent' },
];

const ENV = {
  cwd: '/repo',
  repo: '/repo',
  harnesses: HARNESSES,
  herdr: 'ok' as const,
  budgetDefaultTokens: 5_000_000,
};

function memStorage(): Storage {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => (map.has(k) ? map.get(k)! : null),
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
    key: (i: number) => [...map.keys()][i] ?? null,
    get length() {
      return map.size;
    },
  } as Storage;
}

describe('deck model selector', () => {
  it('pill click opens the dialog for that harness', () => {
    const s0 = dialogReducer({ openFor: null }, { type: 'close' });
    const s1 = dialogReducer(s0, { type: 'pill_click', harnessId: 'codex' });
    expect(s1.openFor).toBe('codex');
  });

  it('harness pills expose an affordance to open the model list', () => {
    const html = renderToStaticMarkup(<FirstDirection goal="ship it" env={ENV} onPick={() => {}} />);
    expect(html).toContain('who rides?');
    // Every pill carries the hook the dialog opens from; no pill may be dead.
    for (const h of HARNESSES) {
      expect(html).toContain(h.label);
    }
    expect(html).toContain('data-action="open-model-select"');
  });

  it('search filters the model list', () => {
    const models: CatalogModel[] = [
      { id: 'sonnet', name: 'Sonnet', provider: 'anthropic', harness: 'claude', visible: true },
      { id: 'haiku', name: 'Haiku', provider: 'anthropic', harness: 'claude', visible: true },
    ];
    expect(filterModels(models, 'hai').map((m) => m.id)).toEqual(['haiku']);
    expect(filterModels(models, '').length).toBe(2);
    expect(filterModels(models, 'SONNET').map((m) => m.id)).toEqual(['sonnet']);
  });

  it('groups by provider and hides invisible models', () => {
    const models: CatalogModel[] = [
      { id: 'a', name: 'A', provider: 'anthropic', harness: 'claude', visible: true },
      { id: 'b', name: 'B', provider: 'anthropic', harness: 'claude', visible: false },
      { id: 'c', name: 'C', provider: 'openai', harness: 'codex', visible: true },
    ];
    const groups = groupModels(visibleModels(models));
    expect(groups.map((g) => g.provider).sort()).toEqual(['anthropic', 'openai']);
    expect(groups.flatMap((g) => g.models).map((m) => m.id).sort()).toEqual(['a', 'c']);
  });

  it('variant row appears only when variants exist', () => {
    const withVariants: CatalogModel = {
      id: 'gpt-5',
      name: 'GPT-5',
      provider: 'openai',
      harness: 'codex',
      visible: true,
      variants: [
        { id: 'low', label: 'Low effort', flag: '--effort' },
        { id: 'high', label: 'High effort', flag: '--effort' },
      ],
    };
    const plain: CatalogModel = { id: 'sonnet', name: 'Sonnet', provider: 'anthropic', harness: 'claude', visible: true };
    expect(shouldShowVariantRow(withVariants)).toBe(true);
    expect(shouldShowVariantRow(plain)).toBe(false);

    const withHtml = renderToStaticMarkup(
      <ModelDialog harnessId="codex" open onClose={() => {}} onSelect={() => {}} selected={undefined} initialModelId="gpt-5" />,
    );
    expect(withHtml).toContain('data-action="variant-row"');
    const withoutHtml = renderToStaticMarkup(
      <ModelDialog harnessId="claude" open onClose={() => {}} onSelect={() => {}} selected={undefined} initialModelId={plain.id} />,
    );
    expect(withoutHtml).not.toContain('data-action="variant-row"');
  });

  it('static catalog covers every harness pill with at least one varianted model somewhere', () => {
    for (const h of HARNESSES) {
      expect(STATIC_CATALOG.some((m) => m.harness === h.id), `no catalog entry for ${h.id}`).toBe(true);
    }
    expect(STATIC_CATALOG.some((m) => (m.variants ?? []).length > 0)).toBe(true);
    expect(STATIC_CATALOG.some((m) => !(m.variants ?? []).length)).toBe(true);
  });

  it('selection persists across reload via the deck store family (localStorage)', () => {
    const storage = memStorage();
    const sel: ModelSelection = { harnessId: 'codex', modelId: 'gpt-5', variantId: 'high' };
    saveSelection(storage, sel);
    // A fresh load — same key family as agentworth_theme — returns what was saved.
    expect(loadSelection(storage, 'codex')).toEqual(sel);
    expect(loadSelection(storage, 'claude')).toBeUndefined();
  });

  it('quota states render dialogs, never raw errors', () => {
    for (const status of ['unpaid', 'over_quota', 'missing_key'] as const) {
      const copy = quotaDialogContent(status);
      expect(copy.title.length).toBeGreaterThan(0);
      expect(copy.body.length).toBeGreaterThan(0);
      const html = renderToStaticMarkup(<QuotaDialog status={status} onClose={() => {}} />);
      expect(html).toContain('role="dialog"');
      expect(html).toContain(copy.title);
      // No raw machine code leaks into the dialog.
      expect(html).not.toContain('ERR_');
      expect(html).not.toContain('status 402');
    }
  });

  it('model dialog groups, tags free/cost, and searches', () => {
    const html = renderToStaticMarkup(
      <ModelDialog harnessId="claude" open onClose={() => {}} onSelect={() => {}} selected={undefined} initialSearch="sonnet" />,
    );
    expect(html).toContain('role="dialog"');
    expect(html).toContain('Sonnet');
    expect(html).not.toContain('Haiku');
    expect(html).toMatch(/free|paid|cost/i);
  });
});
