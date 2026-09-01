import { useEffect, RefObject } from "react";

/**
 * Scroll-reveal as progressive enhancement — design.md "Motion rules".
 * Content is visible by plain CSS (figure.diagram / .term-grid render
 * normally) unless this hook can add .reveal-pending AND guarantee it will
 * also add .is-visible later, so a slow, blocked, or misfiring observer can
 * never leave content stuck invisible. revealAll() is the hard-timeout
 * backstop for exactly that case.
 */
export function useScrollReveal(rootRef: RefObject<HTMLElement>) {
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    if (!("IntersectionObserver" in window)) return;

    const targets = Array.from(root.querySelectorAll<HTMLElement>("figure.diagram, .term-grid"));
    if (targets.length === 0) return;

    let revealed = false;
    const revealAll = () => {
      if (revealed) return;
      revealed = true;
      targets.forEach((el) => el.classList.add("is-visible"));
    };

    targets.forEach((el) => el.classList.add("reveal-pending"));

    const io = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-visible");
            io.unobserve(entry.target);
          }
        });
      },
      { threshold: 0.2, rootMargin: "0px 0px -10% 0px" }
    );
    targets.forEach((el) => io.observe(el));

    const timeout = window.setTimeout(revealAll, 2500);

    return () => {
      io.disconnect();
      window.clearTimeout(timeout);
    };
  }, [rootRef]);
}
