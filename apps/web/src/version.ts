/**
 * The AgentWorth CLI's shipped version — mirrors [workspace.package].version
 * in the repo's root Cargo.toml (and packages/agentworth/package.json, the
 * npm wrapper `npx agentworth` runs). Update there first, then here; a
 * single constant keeps the version badge from drifting across the four
 * places it's displayed in the UI.
 */
export const APP_VERSION = "0.1.7";
