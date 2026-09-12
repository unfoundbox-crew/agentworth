# AgentWorth dashboard

The optional localhost UI. Built with `npm run build`, embedded into the `agentworth` binary
via `rust-embed` (see AGENTS.md item 5) — the CLI ships a stub UI if this isn't built first.

## The Rust -> TypeScript contract loop

The recurring bug class here is Rust and TypeScript drifting apart on an API response shape
with nobody noticing until the UI renders blank cells or dashes (AGENTS.md item 2 has three
examples from one day). `apps/cli/tests/api_contract_fixtures.rs` and
`src/types/contract.test.ts` close that loop:

1. Change a Rust response type in `apps/cli/src/server/routes.rs` or one of the crates it
   returns.
2. Regenerate the fixture: `UPDATE_API_FIXTURES=1 cargo test -p agentworth-cli api_contract`.
   This writes one representative JSON value per route under `src/types/__fixtures__/`,
   built from the real struct — a field you removed from the struct won't compile, so you
   can't regenerate past a shape change without fixing the fixture-building code first.
3. Commit the updated fixture.
4. Run `npm run typecheck` here. `contract.test.ts` imports every fixture and checks it
   `satisfies` the corresponding TS type in `src/types/index.ts` — that's what tells you
   what the TS side needs to catch up to, naming the exact field.

Both halves run in CI on any change under `apps/dashboard/**` or `apps/cli/**` (see
`.github/workflows/ci.yml`): the Rust fixture tests run as part of `cargo test --workspace`,
and `npm run typecheck` runs as its own dashboard CI step.
