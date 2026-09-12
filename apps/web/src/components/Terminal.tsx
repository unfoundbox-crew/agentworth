import React, { useEffect, useRef, useState } from "react";

const REVEAL_MS = 6000;

/**
 * A `.plate`-style dark terminal frame. Renders real, pre-captured CLI
 * output as selectable text (never a video) — the caller supplies each
 * output line as a node, with `t-accent` / `t-dim` / `t-bold` spans already
 * applied to carry the CLI's real colour semantics.
 *
 * The full text is always in the DOM (no layout shift, works with no JS).
 * Once mounted, if motion is allowed, lines start hidden and reveal one at
 * a time over ~6s the first time the plate scrolls into view.
 */
export function Terminal({
  command,
  lines,
  ariaLabel,
}: {
  command: string;
  lines: React.ReactNode[];
  ariaLabel: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [anim, setAnim] = useState(false); // true once we've decided to stage-hide lines pre-reveal
  const [revealed, setRevealed] = useState(false);

  useEffect(() => {
    const reduced = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)"
    ).matches;
    if (reduced || !("IntersectionObserver" in window)) return;
    setAnim(true);
  }, []);

  useEffect(() => {
    if (!anim || revealed) return;
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        entries.forEach((e) => {
          if (e.isIntersecting) {
            setRevealed(true);
            io.unobserve(e.target);
          }
        });
      },
      { threshold: 0.3 }
    );
    io.observe(el);
    return () => io.disconnect();
  }, [anim, revealed]);

  const stagger = lines.length > 1 ? REVEAL_MS / lines.length : 0;

  return (
    <div
      className={`term-plate${anim ? " term-anim" : ""}${
        revealed ? " term-in" : ""
      }`}
      ref={ref}
      role="group"
      aria-label={ariaLabel}
    >
      <div className="term-bar">
        <span className="term-dot" />
        <span className="term-dot" />
        <span className="term-dot" />
      </div>
      <pre className="term-body">
        <code>
          <span className="term-line term-cmd">
            <span className="term-prompt">&gt;</span> {command}
          </span>
          {lines.map((line, i) => (
            <span
              className="term-line"
              key={i}
              style={{ ["--term-d" as string]: `${i * stagger}ms` }}
            >
              {line}
            </span>
          ))}
        </code>
      </pre>
    </div>
  );
}
