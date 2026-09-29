import { useState } from 'react';
import { ThemeToggle } from '@ui/ThemeToggle';
import type { Harness, HomeEnv, Rung } from '../protocol';
import { ModelDialog } from './ModelDialog';
import { loadBrowserSelection, saveBrowserSelection, type ModelSelection } from '../model/modelSelection';

const RUNG_DOTS: Record<Rung, string> = {
  said: '●○○○○',
  artifact: '●●○○○',
  test: '●●●○○',
  commit: '●●●●○',
  ci: '●●●●●',
};

/** A budget under a million reads as "500K"; at or above a million, "5M". Matches the strip's own terse style. */
function formatBudget(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(tokens % 1_000_000 === 0 ? 0 : 1)}M`;
  if (tokens >= 1_000) return `${Math.round(tokens / 1000)}K`;
  return String(tokens);
}

/**
 * The typed line became a strip. One question -- who rides -- with the first harness
 * pre-focused so Enter is the instant default; the choice stays visible and arrow keys move
 * it. Nothing is sent to herdr until a harness is picked (decision 2 and 3 of the first-run
 * brief). The area auto-fills from what the environment already knows and stays editable.
 */
export function FirstDirection({
  goal,
  env,
  onPick,
}: {
  goal: string;
  env: HomeEnv;
  onPick(harness: Harness, area: string, selection?: ModelSelection): void;
}) {
  const [area, setArea] = useState(env.repo ?? env.cwd);
  const [focusIndex, setFocusIndex] = useState(0);
  const [openFor, setOpenFor] = useState<string | null>(null);
  const done: Rung = 'test';
  const harnesses = env.harnesses;

  function openModels(index: number) {
    const h = harnesses[index];
    if (h) setOpenFor(h.id);
  }

  function choose(harness: Harness, selection: ModelSelection) {
    saveBrowserSelection(selection);
    setOpenFor(null);
    onPick(harness, area, selection);
  }

  return (
    <div className="col-start-1 row-span-4 relative">
      <div className="absolute left-6 top-5 font-sans text-sm font-medium text-dim tracking-tight">home</div>
      <div className="absolute right-6 top-4">
        <ThemeToggle />
      </div>

      <div
        className="deck-card-wide absolute left-1/2 top-[19%] -translate-x-1/2 rounded-lg bg-panel border border-line p-4 enter"
        style={{ borderLeft: '4px solid var(--mv-accent)' }}
      >
        <div className="text-[15px] font-medium text-ink">{goal}</div>
        <div className="mt-3 flex gap-6 text-[11px] text-muted">
          <div>
            area:{' '}
            <input
              value={area}
              aria-label="area this direction owns"
              onChange={(e) => setArea(e.target.value)}
              className="bg-transparent outline-none border-b border-dashed border-line text-text w-64 max-w-full"
            />
          </div>
          <div>
            done: <span className="text-text">{done}</span> <span className="tracking-wider">{RUNG_DOTS[done]}</span>
          </div>
          <div>
            budget: <span className="text-text">{formatBudget(env.budgetDefaultTokens)}</span>
          </div>
        </div>
      </div>

      <div className="deck-card-wide absolute left-1/2 top-[38%] -translate-x-1/2 text-center">
        <div className="text-[13px] text-dim">who rides?</div>

        {env.herdr !== 'ok' ? (
          <div className="mt-3 text-[11px] text-warn">
            herdr not found &middot; install it: <a href="https://herdr.dev" className="underline">herdr.dev</a>
          </div>
        ) : harnesses.length === 0 ? (
          <div className="mt-3 text-[11px] text-dim">no harness found on PATH</div>
        ) : (
          <>
            <div
              className="mt-3.5 flex justify-center gap-2.5 flex-wrap"
              role="radiogroup"
              aria-label="who rides"
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === 'ArrowRight') setFocusIndex((i) => (i + 1) % harnesses.length);
                else if (e.key === 'ArrowLeft') setFocusIndex((i) => (i - 1 + harnesses.length) % harnesses.length);
                else if (e.key === 'Enter') openModels(focusIndex);
              }}
            >
              {harnesses.map((h, i) => (
                <button
                  key={h.id}
                  type="button"
                  data-action="open-model-select"
                  data-harness={h.id}
                  onClick={() => openModels(i)}
                  onFocus={() => setFocusIndex(i)}
                  className="rounded-md px-4 py-2 text-[13px]"
                  style={
                    i === focusIndex
                      ? { border: '2px solid var(--mv-accent)', color: 'var(--mv-ink)' }
                      : { border: '1px solid var(--mv-border)', color: 'var(--mv-muted)' }
                  }
                >
                  {h.label}
                </button>
              ))}
            </div>
            <div className="mt-3.5 text-[11px] text-dim">starts in a pane you can watch</div>
          </>
        )}
      </div>

      <div className="absolute left-1/2 top-[52%] -translate-x-1/2 text-[11px] text-dim text-center">
        change any of this later &middot; nothing is sent until you pick
      </div>

      <div className="absolute left-6 right-6 bottom-14 border-t border-line" />

      {openFor ? (
        <ModelDialog
          harnessId={openFor}
          open
          onClose={() => setOpenFor(null)}
          onSelect={(sel) => {
            const h = harnesses.find((x) => x.id === openFor);
            if (h) choose(h, sel);
          }}
          selected={loadBrowserSelection(openFor)}
        />
      ) : null}
    </div>
  );
}
