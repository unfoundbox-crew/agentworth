# Voice

Status: proposed, 2026-09-06. Nothing built. Answers `voice-brief.md`. Every
fact below names where it was read or the command that measured it, all on
2026-09-06; a claim without a source is marked as a call, not a fact.

## The one-line version

A phone on the nightstand that hears "Hey Archie, what's up?" and answers
from the index in the time a person would, with no cloud, no browser, no
root, and a receipt for every sentence it speaks.

## What was measured before any call was made

| Fact | Source |
| :--- | :--- |
| No GPU in the fleet. lenovo is an i5-6200U, 4 threads, 15 GB, Ubuntu 24.04, `nvidia-smi` absent. It runs ollama with `gemma4` (9.6 GB) and `gemma3:4b` (3.3 GB), SearXNG at 100.99.50.84:8888, and Docker | `ssh lenovo` probes |
| This Mac is an M1 Max, 32 GB, no inference tooling, and the fan rule bars sustained compute on it | `sysctl`, `~/.claude/CLAUDE.md` |
| OnePlus 5 stops at Android 10, API 29. From Android 9 "apps in the background cannot access the microphone"; a foreground service is required. Whether a shell-UID binary is exempt is documented nowhere | Wikipedia OnePlus 5; developer.android.com Android 9 changes |
| AAudio/Oboe low-latency exclusive streams exist from API 26. `AcousticEchoCanceler.isAvailable()` is a per-device runtime check; Oboe issues document devices where `VoiceCommunication` breaks | Oboe FullGuide; google/oboe#1110, #2123 |
| sherpa-onnx runs streaming Zipformer ASR, Piper/VITS/Kokoro TTS, Silero VAD and keyword spotting on ARM CPUs, ships Android example APKs and a Rust binding; a streaming model documents 160 ms latency, KWS 160–320 ms per chunk | k2-fsa/sherpa-onnx README and docs |
| Wyoming is one JSON header line plus optional binary payload over TCP; events cover audio, wake, ASR, TTS, VAD and satellite control. There is no interrupt or cancel event anywhere in the protocol | rhasspy/wyoming README; wyoming-satellite README |
| openWakeWord trains a custom phrase from synthetic speech in under an hour, exports ONNX/tflite; a Pi 3 core runs 15–20 models in real time | dscripka/openWakeWord README |
| Piper is real time on a Pi 5 CPU and streams raw PCM by sentence; wyoming-piper has `--streaming` | rhasspy/piper, rhasspy/wyoming-piper |
| Human turn-taking gap: median about 100 ms, mode 0–200 ms across languages | Stivers et al., PNAS 2009 |
| SearXNG answers `?q=…&format=json` with `results[].title/content/publishedDate/engine` | `curl` from this Mac |
| `load_wake` lives in `apps/cli/src/wake/mod.rs`, not in `agentworth-storage` | `git show origin/main` |
| Tailscale: mac, air, iphone-13, lenovo (100.99.50.84) online. No phone is attached over adb today | `tailscale status`, `adb devices` |

Two consequences the brief did not have:

1. **450 ms through a language model is not available at $0 on this fleet.** A 4-thread laptop CPU gives seconds of first-token latency on a 4B model, and the only Apple silicon is the machine the fan rule protects. So the budget is met a different way: the things the brief asks for at breakfast are rows, not prose.
2. **The "compiled daemon" has to live inside an app.** Stock Android 10 holds the mic for a foreground service and nothing else. The native core stays native; the shell around it is a Kotlin foreground service.

## The shape

```
 OnePlus 5 (thick satellite, APK: Kotlin foreground service + Rust core)
   mic ─▶ AEC ─▶ KWS "hey archie" ─▶ Silero VAD ─▶ streaming ASR ──── transcript ──▶
   spk ◀─ Piper TTS (on device, sentence chunks) ◀────────────── speak / cancel ◀─┐
                              Wyoming over TCP, Tailscale or LAN                    │
 ┌───────────────────── the villa (Docker on the Mac, no secrets mounted) ─────────┴─┐
 │ archie voice: Wyoming server ─▶ intent grammar ─▶ briefing composer ─▶ speakable  │
 │   reads ~/.agentworth (rw), ~/.claude/projects, ~/.gemini/…/brain, ~/.config/herdr,│
 │   ~/code (all ro); tools: SearXNG on lenovo, GitHub public API, ollama on lenovo   │
 │   every turn is a row in voice_turns with its stage timestamps and its receipts    │
 └───────────────────────────────────────────────────────────────────────────────────┘
```

## 1. Protocol: Wyoming, plus two events

Wyoming, unchanged, for the audio plane. It is the standard the brief names,
it is what a second satellite (an ESP32 box, a Pi, Home Assistant) already
speaks, and its framing costs nothing. The phone is a *thick* satellite: it
runs wake word, VAD, ASR and TTS itself and sends text, because a 4-thread CPU
across a network hop cannot meet the budget and a Snapdragon 835 next to the
speaker can.

| Direction | Events used | Note |
| :--- | :--- | :--- |
| satellite → brain | `describe`, `streaming-started`, `voice-started`, `voice-stopped`, `transcript` (final text), `streaming-stopped`, `audio-start/chunk/stop` (thin mode only) | `transcript` from the satellite is the thick mode; the brain also accepts raw audio from a plain `wyoming-satellite` and runs STT on lenovo (tier B) |
| brain → satellite | `info`, `run-satellite`, `pause-satellite`, `synthesize` (one sentence per event, in order), `audio-start/chunk/stop` (thin mode) | one `synthesize` per sentence is what lets playback start before the answer ends |
| extension, both ways | `archie.turn` `{turn_id, stage, at_ms}` and `archie.cancel` `{turn_id, reason}` | the two things Wyoming lacks: a turn id to hang receipts on, and a cancel |

Custom event names are legal in the framing (any `type`), and a client that
does not know them ignores them. Transport is Wyoming's own TCP, bound on the
Tailscale interface only, port 10700.

## 2. Barge-in

The protocol cannot interrupt, so the satellite does, locally, and tells the
brain afterwards.

1. During playback the mic stays open through the AEC path (`InputPreset::VoiceCommunication`, and `AcousticEchoCanceler` when `isAvailable()`).
2. KWS and VAD keep running on the echo-cancelled stream. A wake word, or voice energy above a raised threshold for 150 ms, is a barge-in.
3. The satellite flushes its AAudio output stream (`requestStop`, drain not waited), drops every queued `synthesize`, and sends `archie.cancel {turn_id, reason: "barge-in"}` and `voice-started`. This is one local decision; it does not wait for the network.
4. The brain cancels the turn: the TTS queue, the SearXNG request and the model stream share one `CancellationToken`, and the turn's row records `cancelled_at`.

Devices where AEC is not available fall to half duplex: the mic is muted
during playback and barge-in is the wake word only, spoken after playback.
The satellite says which mode it is in at `describe`, and `archie voice
status` prints it. That is measured on the phone in phase 0, not assumed.

## 3. Components

| Where | What |
| :--- | :--- |
| `crates/wake` (moved out of `apps/cli/src/wake`, with `handoff`) | so the brain links `load_wake` without linking the CLI; the brief's "in `agentworth-storage`" is not where it is |
| `crates/voice` | the brain, pure where it can be: intent grammar, briefing composer, speakable renderer, receipts. Wyoming codec and server on tokio. Tool clients: SearXNG, GitHub public REST, an OpenAI-compatible chat client for ollama |
| `apps/cli` | `archie voice run` (the brain), `archie voice ask "<text>"` (a turn with no audio, for tests and for the terminal), `archie voice say "<text>"` (drives a satellite's TTS), `archie voice turns` (the receipts), `archie voice status` |
| `apps/android/satellite` | Kotlin foreground service, `cargo-ndk` Rust core over Oboe and sherpa-onnx (C API), models bundled: KWS or openWakeWord "hey archie", Silero VAD, a streaming Zipformer, a Piper voice |
| `deploy/villa/` | Dockerfile and compose for the brain on the Mac with exactly the brief's mounts, read-only where it says, and `~/.ssh`, `~/.aws`, `~/.doppler`, `~/.zshrc` absent |

`crates/voice` never scans, never writes the index except `voice_turns`, and
never opens the loop socket: it reads `agent_state` from the same SQLite in
WAL mode, which is enough for "who is working".

## 4. What Archie says, and where each sentence comes from

The morning briefing is deterministic. Rows go through templates and a
speakable renderer (no markdown, numbers read as words, three sentences per
answer, then "more?"). No model is in this path, which is why it is fast and
why every sentence has a receipt.

| Intent | Grammar | Source | Receipt spoken on request |
| :--- | :--- | :--- | :--- |
| what's up / brief me | default at wake | `list_agent_states`, newest primary session per repo via `load_wake`, `support_set` drift, open PRs from `api.github.com/repos/<owner>/<repo>/pulls` (public repos only; private ones are "not here" because no token is mounted), SearXNG news for a configured watchlist | session ids, PR numbers, result URLs |
| who is working | "who's working", "anyone busy" | `agent_state` | session ids and repos |
| what drifted | "what moved", "drift" | `session drift` over the newest sessions | paths and writer sessions |
| PRs | "pull requests", "what's waiting" | GitHub public REST | numbers, titles, CI state |
| news about X / what happened with X | "news", "what happened", "latest" | SearXNG JSON, top three titles read as they are | the URLs |
| open question | anything else | the model, only when `--model` names one | "from the model, not the index" is said aloud |
| stop / never mind | any time | cancel | — |

The model path exists and is honest about itself. `archie voice run --model
ollama/gemma3:4b@100.99.50.84` names the model, prints its cost line (watts
on lenovo, no bill), and Archie prefixes those answers with "the model says".
Without the flag, open questions get "I don't have a model configured for
that", which is the AGENTS.md rule kept in a voice.

## 5. Latency budget

Measured from the end of the person's speech, which is where the 450 ms of
the brief and the 100–200 ms of Stivers both count from.

| Stage | Where | Target | Why it is credible |
| :--- | :--- | ---: | :--- |
| endpoint decision | phone, Silero VAD | 200 ms | a fixed silence window; shorter than the usual 300 because the ASR is streaming and the text already exists |
| final transcript | phone, streaming Zipformer | 50 ms | the model has been decoding during speech; 160 ms chunk latency is documented |
| intent + briefing | villa, from a warm cache the brain refreshes every 30 s and on every `agent_state` change | 20 ms | rows, not prose |
| network | Tailscale on LAN | 10 ms | measured, not assumed, in phase 0 |
| first audio | phone, Piper first sentence | 150 ms | Piper is real time on a Pi 5; the first sentence is short by construction |
| **briefing, first sound** | | **430 ms** | |
| lookup (SearXNG) | | 600–1,500 ms | Archie says "looking" at 200 ms, which is itself a turn |
| model answer, first sentence | lenovo CPU, `gemma3:4b`, prompt cache warm | 2–6 s | said aloud as "let me think"; not on the breakfast path |
| tier B (plain satellite, STT on lenovo CPU) | | 3–5 s | faster-whisper CPU figures; a fallback, not the product |

Every stage stamps `archie.turn` and lands in `voice_turns`, so the budget is
a report, not a promise: `archie voice turns` prints the p50 and p95 of each
column. The 430 ms row is a target until phase 0 measures the phone.

## 6. Failure modes, each with what Archie says

| Failure | Behaviour |
| :--- | :--- |
| brain unreachable | the phone says "Archie is offline" with its own TTS; retries with backoff; the wake word still answers so the person knows the mic works |
| AEC unavailable on the phone | half duplex, announced once at start and shown in `voice status` |
| SearXNG down | "search is down", with the host named; the briefing skips the news line |
| no `--model` | "I don't have a model configured for that" |
| ollama down | "the model is not answering", and the turn row says so |
| GitHub rate limit (60/h unauthenticated) | PR line says "last checked at hh:mm" and serves the cache |
| Tailscale down | LAN address as a second `--listen`; else the phone reports offline |
| phone in Doze | foreground service with a wake lock while docked; the phone lives on a charger |
| index stale | the briefing ends with "index last scanned at hh:mm" when older than an hour |

## 7. Schema and grammar

One table, `CREATE TABLE IF NOT EXISTS` like the rest:

`voice_turns(turn_id TEXT PRIMARY KEY, satellite TEXT, started_at, endpoint_at, transcript_at, first_audio_at, finished_at, cancelled_at, transcript TEXT, intent TEXT, model TEXT, receipts TEXT /* JSON: session ids, PR numbers, URLs */, mode TEXT /* duplex|half */)`.

Grammar: a new top-level noun `voice` with verbs `run`, `ask`, `say`,
`turns`, `status`. It is a long-running process like `serve` and `mcp`, which
is why it is its own noun rather than a `serve` flag. MCP gains nothing: the
voice brain is a client of the index, not a tool for agents.

## 8. Phases, and the gate on each

| Phase | What | Gate |
| :--- | :--- | :--- |
| 0, measure | sideload sherpa-onnx's example APKs on the OnePlus 5; a ten-line Kotlin probe for `AcousticEchoCanceler.isAvailable()` and Oboe exclusive-mode latency; Tailscale on the phone; ping to the Mac | numbers for KWS, ASR final, TTS first chunk, AEC yes/no, network |
| 1, the brain in text | `crates/wake` move, `crates/voice` grammar, composer, renderer, receipts, `archie voice ask` | the briefing reads right in the terminal and every sentence has a receipt |
| 2, the wire | Wyoming server, tier B with `wyoming-satellite` on lenovo speaking through lenovo's Piper and faster-whisper | a plain satellite completes a turn end to end |
| 3, the satellite | the APK, thick mode, barge-in | the 430 ms row measured on the phone, five mornings in a row |
| 4, the villa | Dockerfile, mounts, no secrets, Tailscale reachability from Docker Desktop verified | `archie voice status` from inside the container sees the index and lenovo |
| 5, the model | `--model` path, cost line, "the model says" | an open question answered, with the row showing which model |

Phase 0 needs the phone on a cable and Developer Mode on; nothing else in this
spec needs a hand on hardware.

## What stays true

No upload: the only outbound requests are to lenovo over the tailnet and to
GitHub's public API for public repos. No bill. No browser anywhere. No root,
no ROM, no bootloader. No model without a flag that names it. Every spoken
sentence traces to a row or a URL, and the turn it came from is a row too.

## Cost, in tokens

About 3M tokens across nine lanes: the `crates/wake` move 150k, the brain
600k, the Wyoming codec and server 300k, the Android core 700k, the Kotlin
shell 250k, the villa 150k, the model path 200k, phase-0 probes 150k, review
and gates 500k. Phase 0 is cheap and decides whether phase 3 is the phone or
a different satellite.

## Open questions

- The fleet has no GPU. The model tier is a laptop CPU at 2–6 s, spoken
  honestly. A GPU on lenovo, or lifting the fan rule for a single 8B model on
  the M1 Max, moves that row to under a second. That is a purchase or a rule
  change, not an architecture change.
- "Hey Archie" itself: openWakeWord's synthetic training is documented; whether
  sherpa-onnx's English keyword models spot it without training is not. Phase
  0 tries both.
- Does the phone join the tailnet, or does the brain also listen on the LAN?
  The spec allows both; phase 0 measures which is quieter.
