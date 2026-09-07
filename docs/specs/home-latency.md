# home latency: web deck vs terminal, measured

Status: measured 2026-09-07. Answers one question: typing into the home deck
(React + Vite, WebSocket to a gateway) versus typing into Ghostty/iTerm2 —
is the deck worse, by how much, and where does the time go.

Machine: MacBook Pro, Apple M1 Max, 32 GB RAM, macOS 27.0 (26A5425a).
Deck build: `apps/home` at `e1381ff` (main, PR #152, "the elastic deck").

## What changed the question mid-measurement

The brief assumed the deck's dock input didn't exist yet — `docs/specs/home.md`
still says "concept locked, not built" as of this session's start. It shipped
on main while this lane was running (`e1381ff`, PR #152): `apps/home/src/deck/
Dock.tsx` is a real controlled `<input>` wired to the real store and gateway.
So every number below is measured against the actual shipped `Dock.tsx`, not
a stand-in — except the frame-rate test, which drives 50 fabricated
`Direction` frames and 100 fabricated personas through the real store reducer
and the real `Deck.tsx` render path (the fixture doesn't ship at that volume).

## Method

**Deck, keystroke to paint.** Built `apps/home` (`HOME_GATEWAY` baked to a
local mock), served the real `dist/` from a tiny static+WebSocket server that
speaks the mock gateway's own protocol (copied verbatim from `src/mock/
server.mjs`, same fixture, same frame shapes — the app never knows it isn't
`archie serve`). Drove it headless with Playwright (Python, already installed
under `~/miniconda3`, Chromium 1243). A capture-phase `keydown` listener on
`window` (added before the bundle runs, so it always fires before React's own
handlers) times each keystroke; a capture-phase `input` listener schedules a
`requestAnimationFrame` and times that — this brackets native key handling +
React 18's synchronous commit + the browser's own paint scheduling. 200
keystrokes into the dock, opened for real via the app's own `/` shortcut and a
real click, matching how a person would open it.

**Transport (steer round trip).** Wrapped `window.WebSocket` before the
bundle loads to grab the instance the app's own `ws/client.ts` creates, then
timed `steer` → `direction` ack over the real socket, 200 round trips, mock
gateway on localhost (no network hop — this isolates JSON parse + reducer +
one hop of event-loop scheduling, not a real network RTT).

**Real path, one steer.** `herdr agent prompt probe-haiku "reply with one
word" --wait`, wall clock via `date +%s%N`, exactly once, against the
dedicated `probe-haiku` Haiku pane (per `docs/specs/home.md`'s testing rule —
never poke a live partner pane). This is herdr submit → Claude Haiku's own
generation → settle-to-done; it is not decomposable into gateway vs model
time from outside.

**Frame rate under load.** A second mock instance serves a fixture edited to
50 directions / 100 personas (matching the brief's numbers). Fed the real
`Deck.tsx`/store a real `direction` frame every 100ms for 5s over the wire
(the mock's own `send`, not a synthetic call), sampled every
`requestAnimationFrame` for 5.3s, computed frame-to-frame intervals.

**Terminal, keystroke to paint.** No latency-measurement tool installs
non-interactively on this machine: `brew info typometer` returns no formula,
and typometer's actual method (video capture of the physical screen, frame by
frame) doesn't run headless at all — it needs a camera or a screen-capture
rig pointed at a real display, which this lane doesn't have permission to
drive (browser/screen automation here is measurement-only, and the owner's
CLAUDE.md bars this session from touching a browser for anything but the deck
harness). Published numbers only, cited below, explicitly not measured on
this machine.

## Results — measured on this machine

| Measurement | n | p50 | p95 | p99 | min | max |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| Dock: keydown → next paint (ms) | 200 | 4.4 | 7.9 | 8.5 | 0.3 | 35.3 |
| WS: steer → direction ack (ms) | 200 | 0.7 | 3.4 | 4.8 | 0.3 | 5.3 |

**Frame rate, 50 directions / 100 personas, one `direction` frame every
100ms for 5s:** 583 frames in 5.29s, mean interval 9.08ms, p95 9.5ms, 2
frames over 16.7ms (dropped a 60Hz frame), 430 of 582 intervals over 8.3ms
(this machine renders headless Chromium at roughly a 110Hz internal cadence,
not 60 or 120 — dropped-vs-120Hz isn't a clean binary here; see caveat below).

**Real round trip, one steer to `probe-haiku`:** 2193ms wall clock, herdr
submit through Claude Haiku's reply through settle-to-done. This is the
number a person actually waits for after pressing Enter on a real steer — it
dwarfs both numbers above by roughly 250-3000x, because it is bounded by the
model's own generation time, not the deck.

## Results — published, not measured here

| Terminal | Latency (ms) | Method | Hardware/OS | Source |
| :--- | ---: | :--- | :--- | :--- |
| iTerm2 | 44 (p50, idle) – 81 (p99, load) | 10k keypresses, custom harness | 2014 MacBook Pro, OS X 10.12 | [Dan Luu, "Terminal latency"](https://danluu.com/term-latency/) |
| iTerm2 | 87.5 (GPU: 62.5) | "Is It Snappy?" slow-motion video | 2021 MacBook Pro M1 Pro, macOS 12.4 | [Luke Harris, "Measuring terminal latency"](https://dev.to/lkhrs/measuring-terminal-latency-26m7) |
| kitty | 29.2–37.5 | same as above | same as above | same |
| Ghostty | ~24 avg, ~41 max | Typometer | Debian Linux, Ghostty 1.2.3, no load | [ghostty-org/ghostty discussion #4837](https://github.com/ghostty-org/ghostty/discussions/4837) |

None of these are Ghostty-on-macOS or run on an M1 Max, so they set an order
of magnitude, not a number to subtract from. Several other pages that turned
up in search (petronellatech.com, tech-insider.org, news.creeta.com, biggo.com
— citing figures from 1.2ms to 13ms for Ghostty) read as SEO/AI-generated
content, not primary measurements, and are not used here.

## NOT CONFIRMED

- No terminal latency number measured on this machine, this session, for
  either Ghostty or iTerm2 — typometer doesn't install non-interactively here
  and its method needs a screen-capture rig this lane isn't set up to drive.
- Whether headless Chromium's ~110Hz `requestAnimationFrame` cadence in this
  sandbox matches the cadence a real, on-screen Ghostty Canary/Chrome window
  gets on this same machine — not tested against a real display.
- Whether `archie serve`'s real Rust gateway (not yet built) matches the
  mock's near-zero JSON round trip — the mock has no herdr subprocess, no I/O,
  and no real network hop; this is a lower bound, not the shipped gateway's
  number.

## Conclusion

The deck's own overhead — keystroke to painted character, WS round trip to
an ack — sits at single-digit milliseconds (p50 4.4ms, p99 8.5ms for typing;
p99 4.8ms for the WS ack), inside the range published iTerm2 numbers occupy
on comparable hardware (44–87ms) and Ghostty's own self-reported number
(~24ms avg) — so on the evidence gathered here, the deck's raw input path is
not obviously worse than typing into a terminal, and may be faster, though
the terminal side is published-not-measured on this machine. The time a
person actually feels after pressing Enter is dominated by the model's own
reply, not the deck: one real steer to Haiku took 2193ms end to end, roughly
500x the deck's own p50 latency — so the perceived "typing tax" question is
really a "which model, how fast" question once a steer leaves the dock.
