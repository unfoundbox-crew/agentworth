# AgentWorth home

The deck: a local window for running 5 to 100 coding agents by setting directions and
reading only what needs them. See `docs/specs/home.md` and `apps/home/DESIGN.md` for the
product shape.

## Two ways to run this, for two different people

**A user runs `archie home`.** That is the whole answer to "I ran some npm commands, I
have things installed, now what?" It starts `archie serve --home` on one port, with this
app's built `dist/` compiled into the `archie` binary (the same `rust-embed` mechanism as
`apps/dashboard`, see AGENTS.md item 5), and opens the browser at `/home/`. Nothing in
`archie home` depends on Vite, on `npm run dev`, or on this directory existing on disk at
all — it is baked into the binary at build time.

**A developer runs `npm run dev`.** That starts the Vite dev server on port 5175 with hot
reload, proxying `/ws` and `/api` to `HOME_GATEWAY` / `HOME_API` (real `archie serve --home`
process, or `npm run mock` for a fake one — see `vite.config.ts`). This is for iterating on
the deck itself, never for a user who just wants to see it.

Rebuilding after a change to this app: `npm run build` here, then rebuild `archie` (`cargo
build -p agentworth-cli --bin archie`) so the new `dist/` gets embedded. If `dist/` was
never built, `archie home` fails loudly rather than serving a blank page.
