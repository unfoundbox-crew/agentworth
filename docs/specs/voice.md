# Voice

Status: proposed, 2026-09-06, rewritten the same day after review. Nothing
built. Answers `voice-brief.md`. The first draft put a microphone on an
Android phone; that was wrong in kind, not in detail. Agents do not have
ears. They live in panes, in SQLite, on branches, and what the room needs
from them is to be told things.

## The one-line version

Archie speaks to the room and answers when asked. Two paths, no app, no
microphone daemon, no wiretap: the Mac announces through whatever speaker is
selected, and a Siri Shortcut on the iPhone asks the Mac for the brief and
reads it into an ear.

## What was measured, and what it decides

| Fact | Source | Decides |
| :--- | :--- | :--- |
| No GPU in the fleet; the M1 Max is under the fan rule; lenovo is a 4-thread i5 | `ssh lenovo`, `sysctl` | no language model on the breakfast path; the brief is rows |
| macOS `say` and `afplay` are on every Mac, speak system voices, and play to the selected output device (AirPlay, Bluetooth, the built-in speaker) | the OS | the annunciator is one command, and the room device is any speaker the Mac can reach |
| iOS Shortcuts run "Get Contents of URL" and "Speak Text" with no code; the iPhone is on the tailnet | `tailscale status` shows `iphone-13` | "Hey Siri, fleet brief" is a URL and a voice, zero lines of app |
| `archie serve` binds 127.0.0.1 only | `apps/cli/src/server/mod.rs` | it needs a tailnet listener with a token before a phone can ask it anything |
| A Shortcut cannot be started from another machine | Apple's Shortcuts model | push into the room comes from the Mac, pull comes from the phone; the two never meet in one device |
| The OnePlus 5 is a 2017 phone on Android 10 with no NPU and a runtime-checked AEC | the first draft's research | it is retired from this spec. If it is used at all, it is as a Bluetooth speaker the Mac plays to |

## Push: the annunciator

`archie voice say "<text>"` speaks through the Mac's current output device.
Nothing else in this spec is more complicated than that line.

| Event | Source | What is said |
| :--- | :--- | :--- |
| morning brief | a `launchd` timer or `archie voice brief` by hand | the `session_wake` document for each active repo, rendered speakable: who worked, the last proof, the loose ends, what moved under whom |
| a PR merged or went green | `gh` on the host, polled by `archie voice watch` (public repos need no token; private ones use the host's `gh`, which is fine: this runs on the Mac, not in a villa) | "PR one-nine-six merged" |
| an agent halted or blocked | the loop's `agent_state` and, when the governor lands, `governor_events` | "Louis halted in pane J: same file, four edits, no passing test" |
| drift under a session | `session_drift` on the newest sessions | "two files moved under the studio session, both by the landing session" |

The speakable renderer is the one piece of new code: markdown to speech,
numbers to words, three sentences then stop, session ids never spoken. It
lives in `crates/voice` and is the same renderer the pull path uses.

Quiet hours, a per-event on/off, and the output device are three lines in
`~/.agentworth/voice.toml`. Nothing here listens.

## Pull: the Shortcut

`archie serve --listen tailscale` binds a second listener on the Mac's
tailnet address, and `GET /api/brief?format=speech` returns the speakable
brief as plain text, requiring a bearer token the person generates once with
`archie voice token`. The Shortcut is two actions: Get Contents of URL with
the header, Speak Text. "Hey Siri, fleet brief" is the Shortcut's name.
`/api/brief` takes `repo=` for one repo and `since=` for a window.
`/api/say` is deliberately absent: the phone reads; it does not make the Mac
speak, because the Mac is where the person already is.

## Components

| Where | What |
| :--- | :--- |
| `crates/wake` | `load_wake` and the handoff move out of `apps/cli` so the brief can be composed from a crate |
| `crates/voice` | the speakable renderer, the brief composer, the announcer (a queue in front of `say`, one utterance at a time, later ones wait), the event watchers |
| `apps/cli` | noun `voice`: `say`, `brief`, `watch` (long-running announcer), `token`, `status` |
| `apps/cli/src/server` | the tailnet listener and `/api/brief` |

No new tables. Announcements are rows in `voice_turns(turn_id, at, kind,
text, source_ids JSON)` so `archie voice turns` shows what was said and why.

## Not built, on purpose

- No microphone, no wake word, no VAD, no AEC, no Wyoming, no APK, no
  villa. The first draft's research on those is in its git history and
  stays out of the product.
- No model in the brief. An open question is not a voice feature; it is the
  governor's and the terminal's.
- No Home Assistant integration. The announcer is a queue in front of one
  command; if a HomePod is the room speaker, it is the Mac's AirPlay target.

## Cost

About 350k tokens: the `crates/wake` move 150k, the renderer and announcer
120k, the listener and token 50k, docs and the Shortcut recipe 30k. No hand
on hardware except installing one Shortcut.
