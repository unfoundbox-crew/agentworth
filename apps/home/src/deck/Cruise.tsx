import { ThemeToggle } from '@ui/ThemeToggle';
import { useHome } from '../model/store';

/**
 * Rest state: nothing needs you. The interface tax at rest is one line and a
 * hint, nothing else. No direction has an exception.
 */
export function Cruise() {
  const personas = useHome((s) => s.personas);
  const riding = Object.values(personas).filter((p) => p.presence === 'working').length;
  // No direction carries a predicted-arrival field yet -- there is nothing to
  // estimate from, so this stays "?" rather than a guess. See docs/specs/home.md.
  const nextStop = '?';

  return (
    <div className="col-start-1 row-span-4 relative">
      <div className="absolute left-6 top-5 font-sans text-sm font-medium text-dim tracking-tight">home</div>
      <div className="absolute right-6 top-4">
        <ThemeToggle />
      </div>

      <div className="absolute left-1/2 top-[47%] -translate-x-1/2 -translate-y-1/2 text-center whitespace-nowrap enter">
        <div className="text-sm text-text">
          nothing needs you &middot; {riding} riding &middot; next stop ~{nextStop} min
        </div>
        <div className="mt-3 text-[11px] text-dim">
          press / for a direction &middot; 1&ndash;9 open a console &middot; say it to the chief of staff
        </div>
      </div>

      <div className="absolute left-6 right-6 bottom-14 border-t border-line" />
    </div>
  );
}
