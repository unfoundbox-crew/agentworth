import { useMemo, useState } from 'react';
import {
  costTag,
  filterModels,
  groupModels,
  resolveCatalog,
  shouldShowVariantRow,
  visibleModels,
} from '../model/catalog';
import { quotaDialogContent, type ModelSelection, type ModelStatus } from '../model/modelSelection';

/**
 * Harness pill -> searchable model list, mirroring opencode's
 * `dialog-select-model` (group by provider, free/cost tags, per-model
 * visibility) and `prompt-input`'s variant control (second row only when
 * variants exist). Quota/key failures render as `QuotaDialog`, never raw errors.
 */
export function ModelDialog({
  harnessId,
  open,
  onClose,
  onSelect,
  selected,
  initialSearch = '',
  initialModelId,
}: {
  harnessId: string;
  open: boolean;
  onClose(): void;
  onSelect(sel: ModelSelection): void;
  selected: ModelSelection | undefined;
  initialSearch?: string;
  initialModelId?: string;
}) {
  const [search, setSearch] = useState(initialSearch);
  const [modelId, setModelId] = useState<string | undefined>(initialModelId ?? selected?.modelId);
  const [variantId, setVariantId] = useState<string | undefined>(selected?.variantId);

  const groups = useMemo(() => {
    const scoped = visibleModels(resolveCatalog().filter((m) => m.harness === harnessId));
    return groupModels(filterModels(scoped, search));
  }, [harnessId, search]);

  const active = useMemo(
    () => groups.flatMap((g) => g.models).find((m) => m.id === modelId),
    [groups, modelId],
  );
  const showVariants = active ? shouldShowVariantRow(active) : false;

  if (!open) return null;

  function choose() {
    if (!modelId) return;
    const sel: ModelSelection = { harnessId, modelId, ...(variantId ? { variantId } : {}) };
    onSelect(sel);
  }

  return (
    <div role="dialog" aria-label={`models for ${harnessId}`} className="absolute left-1/2 top-[62%] -translate-x-1/2 w-[720px] rounded-lg bg-panel border border-line p-4 shadow-lg">
      <div className="flex items-center gap-2">
        <input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="search models"
          aria-label="search models"
          className="flex-1 bg-transparent outline-none border-b border-dashed border-line text-text text-[13px] py-1"
        />
        <button type="button" onClick={onClose} aria-label="close model list" className="text-[11px] text-dim px-2 py-1">
          close
        </button>
      </div>

      {groups.length === 0 ? (
        <div className="mt-3 text-[11px] text-dim">no models match</div>
      ) : (
        <div className="mt-3 max-h-56 overflow-y-auto flex flex-col gap-3">
          {groups.map((g) => (
            <div key={g.provider}>
              <div className="text-[10px] uppercase tracking-wide text-dim">{g.provider}</div>
              <div className="mt-1 flex flex-col gap-1">
                {g.models.map((m) => {
                  const tag = costTag(m);
                  const isActive = m.id === modelId;
                  return (
                    <button
                      key={m.id}
                      type="button"
                      onClick={() => {
                        setModelId(m.id);
                        setVariantId(undefined);
                      }}
                      aria-pressed={isActive}
                      className="flex items-center gap-2 rounded-md px-3 py-1.5 text-left text-[13px]"
                      style={
                        isActive
                          ? { border: '2px solid var(--mv-accent)', color: 'var(--mv-ink)' }
                          : { border: '1px solid var(--mv-border)', color: 'var(--mv-muted)' }
                      }
                    >
                      <span className="truncate">{m.name}</span>
                      {tag ? <span className="text-[10px] text-dim shrink-0">{tag}</span> : null}
                    </button>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}

      {showVariants && active ? (
        <div className="mt-3 border-t border-line pt-2" data-action="variant-row" aria-label="model variants">
          <div className="text-[10px] uppercase tracking-wide text-dim">variant</div>
          <div className="mt-1.5 flex gap-2 flex-wrap">
            {(active.variants ?? []).map((v) => (
              <button
                key={v.id}
                type="button"
                onClick={() => setVariantId(v.id)}
                aria-pressed={variantId === v.id}
                className="rounded-md px-3 py-1.5 text-[12px]"
                style={
                  variantId === v.id
                    ? { border: '2px solid var(--mv-accent)', color: 'var(--mv-ink)' }
                    : { border: '1px solid var(--mv-border)', color: 'var(--mv-muted)' }
                }
              >
                {v.label}
              </button>
            ))}
          </div>
        </div>
      ) : null}

      <div className="mt-3 flex justify-end">
        <button
          type="button"
          onClick={choose}
          disabled={!modelId}
          className="rounded-md px-4 py-2 text-[13px] bg-accent text-[var(--mv-accent-contrast)] disabled:opacity-50"
        >
          ride with this model
        </button>
      </div>
    </div>
  );
}

/** Quota/key states render as a dialog with human copy — never a raw error. */
export function QuotaDialog({ status, onClose }: { status: ModelStatus; onClose(): void }) {
  const copy = quotaDialogContent(status);
  return (
    <div role="dialog" aria-label={copy.title} className="absolute left-1/2 top-[62%] -translate-x-1/2 w-[480px] rounded-lg bg-panel border border-line p-4 shadow-lg">
      <div className="text-[13px] font-medium text-ink">{copy.title}</div>
      <div className="mt-1.5 text-[11px] text-muted">{copy.body}</div>
      <div className="mt-3 flex justify-end gap-2">
        <button type="button" onClick={onClose} className="rounded-md px-3 py-1.5 text-[12px] text-muted border border-line">
          {copy.action}
        </button>
        <button
          type="button"
          onClick={onClose}
          className="rounded-md px-3 py-1.5 text-[12px] bg-accent text-[var(--mv-accent-contrast)]"
        >
          close
        </button>
      </div>
    </div>
  );
}
