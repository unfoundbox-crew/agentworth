import { useState } from 'react';
import { ThemeToggle } from '@ui/ThemeToggle';
import type { HomeEnv } from '../protocol';

/**
 * The whole setup, one line: "What are we building?" Rendered when there is no active
 * direction, or summoned with `/`. No accounts, no config -- type it, press enter.
 * apps/home/DESIGN.md's "Quiet state" test applies here too: this is the interface tax at
 * its smallest, not a wizard.
 */
export function FirstRun({ env, onSubmit }: { env: HomeEnv; onSubmit(goal: string): void }) {
  const [text, setText] = useState('');

  function submit() {
    const goal = text.trim();
    if (!goal) return;
    onSubmit(goal);
  }

  return (
    <div className="col-start-1 row-span-4 relative">
      <div className="absolute left-6 top-5 font-sans text-sm font-medium text-dim tracking-tight">home</div>
      <div className="absolute right-6 top-4">
        <ThemeToggle />
      </div>

      <div className="absolute left-1/2 top-[44%] -translate-x-1/2 -translate-y-1/2 w-[560px] text-center enter">
        <input
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') submit();
          }}
          placeholder="What are we building?"
          className="w-full bg-transparent outline-none border-b border-line pb-3 text-center text-2xl text-text placeholder:text-dim"
        />
        <div className="mt-4 text-[13px] text-dim">type it, press enter</div>

        {env.herdr === 'ok' ? (
          env.harnesses.length > 0 && (
            <div className="mt-6 flex justify-center gap-1.5 flex-wrap" aria-label="harnesses found">
              {env.harnesses.map((h) => (
                <span key={h.id} className="border border-line rounded-full px-2.5 py-1 text-[11px] text-muted">
                  {h.label}
                </span>
              ))}
            </div>
          )
        ) : (
          <div className="mt-6 text-[11px] text-warn">
            herdr not found &middot; install it: <a href="https://herdr.dev" className="underline">herdr.dev</a>
          </div>
        )}
      </div>

      <div className="absolute left-6 right-6 bottom-14 border-t border-line" />
    </div>
  );
}
