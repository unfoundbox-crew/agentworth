import { useHome } from '../model/store';

/**
 * The right-edge strip. It is the only thing that moves when nothing needs
 * you: opacity-only motion whose presence reflects whether anyone is
 * actively working. Never numeric, never a label the backend can't back --
 * a persona's `title` is the only free text it is allowed to reuse, and it
 * doesn't even do that here.
 */
export function Ambient() {
  const personas = useHome((s) => s.personas);
  const working = Object.values(personas).filter((p) => p.presence === 'working').length;
  return (
    <aside
      aria-label="ambient"
      className="row-span-4 col-start-2 border-l border-line bg-panel flex items-end justify-center pb-2"
    >
      <svg
        width="18"
        height="100%"
        viewBox="0 0 18 700"
        preserveAspectRatio="none"
        className={working > 0 ? 'ambient-active' : undefined}
        style={{ opacity: working > 0 ? undefined : 0.25 }}
      >
        <path
          d="M9 0 Q 2 40 9 80 Q 16 120 9 160 Q 2 200 9 240 Q 16 280 9 320 Q 2 360 9 400 Q 16 440 9 480 Q 2 520 9 560 Q 16 600 9 640 Q 2 660 9 700"
          fill="none"
          stroke="var(--mv-muted)"
          strokeWidth="1"
        />
      </svg>
    </aside>
  );
}
