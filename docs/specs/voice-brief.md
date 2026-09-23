# Brief: Sovereign Voice Architecture (Archie / JARVIS)
**From**: Donna (`donna-gemini3.8` · Chief of Staff)  
**To**: Louis (`louis-fable5.1` · Senior Partner & Architect, `w9:pJ`)  
**Target Document**: `docs/specs/voice.md` in `agentworth`  
**Date**: 2026-09-06  

---

Louis,

v0.1.21 is shipped. The sensory-motor loop is in the ground. The Managing Partner noticed, and now he is handing you the crown jewel.

Saurabh wants his JARVIS. Not a toy, not a Webview hack, not an enterprise slide deck. Tomorrow morning, he wants to wake up, look at his desk, and say:

> *"Hey Archie, what's up?"*

And Archie answers through the room in natural, low-latency, conversational voice, delivering a live briefing on who is working, what drifted, which PRs are waiting on review, and what happened in the world overnight.

We are not telling you what law to cite or how to index your case file. You are the architect of this firm. You designed `docs/specs/wake.md` and `docs/specs/loop.md`. We want *your* architectural mind to make the calls in `docs/specs/voice.md`.

Here are the facts of the case, the assets on the table, and the Managing Partner's non-negotiable boundaries:

---

### 1. The Client Hardware: OnePlus 5 ("cheeseburger")
- We have a dedicated, spare OnePlus 5 sitting on the nightstand/desk.
- **Specs**: Snapdragon 835, 6–8 GB RAM, dual noise-canceling microphones, hardware audio DSP, 3.5mm headphone jack.
- **Audio Output**: Internal speaker, or plugged via 3.5mm / Bluetooth into an external speaker (like an Echo Dot).
- **Rule**: Zero bricking. No flashing custom ROMs or unlocking bootloaders. It must run in user space (native Android C++ or compiled daemon).

### 2. The Ethos: Direct Hardware & Total Sovereignty
- **No Chrome Middleman**: No browser tabs, no DOM, no Webview wrappers, no sandboxed Web Audio battery throttling. The client must grab raw audio straight from the hardware codec (OpenSL ES / AAudio / ALSA).
- **$0 Recurring Bill**: Zero cloud dependency. No recurring API subscriptions.
- **Open Standards**: The community standard for smart voice satellites is the **Wyoming Protocol** (open peer-to-peer audio protocol over TCP, backing Home Assistant / Rhasspy satellites like Ava Pro and Wyoming-Satellite).
- **Sub-450ms Conversational Latency**: Real humans expect response times under 450ms.
- **Real-Time Barge-In (Interruption)**: If Archie is speaking a 3-sentence summary and Saurabh speaks over him (*"Archie, stop"*), the audio buffer must abort immediately. Hardware Acoustic Echo Cancellation (AEC) must prevent speaker feedback.

### 3. The Sovereign Search Loop (Lenovo SearXNG)
- Archie cannot just be an inward-looking git inspector. If Saurabh asks about external news, model releases, or tech outages, Archie must have search sovereignty.
- We have a dedicated, private **SearXNG instance running locally on Lenovo over Tailscale (`http://100.99.50.84:8888/search?q=...&format=json`)**.
- This must be wired as a first-class tool for Archie. Zero dollars, zero tracking, 100% private.

### 4. Sandboxing: Archie's "Malibu Villa" (Docker Isolation)
- An always-listening assistant capable of executing diagnostics and tools must have a strict blast radius. Saurabh wants Archie running in his own containerized "villa" (Docker).
- **The Telemetry Catch**: Trajectories and databases do *not* live in `~/code`. To prevent Archie from being blind, the container must have selective volume mounts:
  - `~/.agentworth`: Read/Write access to `agentworth.db` (SQLite WAL mode allows seamless concurrent access with host).
  - `~/.claude/projects`: Read-only access to Claude Code trajectories.
  - `~/.gemini/antigravity-cli/brain`: Read-only access to Antigravity sessions.
  - `~/.config/herdr`: Read-only access to Herdr status.
  - `~/code`: Read-only access to repositories for git inspections.
  - **Notice what is EXCLUDED**: `~/.ssh/`, `~/.aws/`, `~/.doppler/`, `~/.zshrc`. Archie has full telemetry vision, but zero access to personal host secrets.

---

### The Assignment

Louis, draft `docs/specs/voice.md`.

You decide:
1. The protocol grammar and wire framing (Wyoming vs. custom socket).
2. The component decomposition across `crates/voice`, `apps/cli` (`archie voice`), and the Docker sandbox.
3. How the pipeline ties into `load_wake` in `agentworth-storage`.
4. The barge-in abort mechanics.
5. The latency budget and failure modes.

Make your architectural calls. Bring us a spec that proves why your name is on the door.
