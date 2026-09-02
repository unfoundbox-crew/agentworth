# What agent traces actually contain, and where open models buy you more

Status: research memo, sources verified 2026-09-02. Context-only, not a decision.

Research memo for AgentWorth. Every claim carries its source. "Unverified" = no primary source found, not false.

## 1. What each harness writes to disk

Verified by parsing 190,573 records across 618 local JSONL files, plus `~/.codex/sessions/`. Structure only.

| Harness | Location / format | What is on disk | Source |
| --- | --- | --- | --- |
| Claude Code | `~/.claude/projects/<slug>/*.jsonl` | `type`s: `assistant`, `user`, `attachment`, `system`, `queue-operation`, `file-history-snapshot`, `mode`, `pr-link`, each with `sessionId`, `uuid`, `parentUuid`, `cwd`, `gitBranch`, `version`, `timestamp`. | measured |
| Claude Code — thinking | content blocks `{type, thinking, signature}` | `thinking` is a **summary**, not raw CoT: "No `display` setting returns the raw chain of thought", and "Summarization is processed by a different model". `signature` is an encrypted copy only the server reads. | https://platform.claude.com/docs/en/build-with-claude/thinking |
| Claude Code — tool results | `tool_result` blocks with `tool_use_id`, `content`, optional `is_error`; plus a `toolUseResult` sidecar on the user record | 34,070 blocks; `is_error` present on 18,039 (True 1,362), **absent on 16,031** — not reliable. Sidecar is typed per tool: Bash → `{stdout, stderr, interrupted, …}`; Edit → `{filePath, oldString, newString, originalFile, structuredPatch}`. | measured |
| Claude Code — compaction | `type: system`, `subtype: compact_boundary`, with `compactMetadata` | `preTokens`, `postTokens`, `cumulativeDroppedTokens`, `preservedMessages`, `preservedSegment`, `preCompactDiscoveredTools`, `trigger`, `durationMs`. Rich and undocumented. | measured |
| Claude Code — usage | `message.usage` on every assistant record (70,502/70,502) | `input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `cache_creation` (per-TTL), `service_tier`, `output_tokens_details`. Same names as the API. | https://platform.claude.com/docs/en/build-with-claude/prompt-caching |
| OpenAI Codex CLI | `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` | `type`s: `session_meta`, `turn_context`, `response_item`, `event_msg`. `turn_context` gives `model`, `effort`, `approval_policy`, `sandbox_policy`, `truncation_policy` — per-turn harness config Claude Code does not write. | measured |
| Codex CLI — persistence rule | `codex-rs/rollout/src/policy.rs` | Persists Message, AgentMessage, Reasoning, FunctionCall/Output, LocalShellCall, Compaction; drops `AdditionalTools`, `CompactionTrigger`, `Other`. Compaction and TokenUsage kept "so we can analyze flows". | https://github.com/openai/codex/blob/main/codex-rs/rollout/src/policy.rs |
| Codex CLI — reasoning | `reasoning` payload shape, measured | `{summary: [{type, text}], content: null, encrypted_content: "<opaque>"}`. **`content` is null on disk** — the summary and an encrypted blob, never the reasoning. | measured |
| Cursor | `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb` (SQLite, `cursorDiskKV`), workspace DBs under `workspaceStorage/*/state.vscdb` | JSON blobs keyed `bubbleId:{composerId}:{messageId}`. **Vendor docs: unverified** — no official schema doc; shape is community reverse-engineering. AgentWorth's adapter reads these paths. | https://github.com/Callum-Ward/cursaves/blob/main/docs/how-cursor-stores-chats.md |
| Gemini CLI | `~/.gemini/tmp/<project_hash>/checkpoints/*.json`, git shadow repo at `~/.gemini/history/<project_hash>` | Official: a git snapshot, full conversation history, and the pending tool call. Continuous per-turn logging is undocumented; the `logs.json` claim is community-only — **unverified**. | https://google-gemini.github.io/gemini-cli/docs/cli/checkpointing.html |
| Aider | `.aider.chat.history.md`, `.aider.input.history`, `.aider.llm.history` | Markdown transcript, input history, and — only with `--llm-history-file` — the actual conversation sent to the LLM. The third is the useful one and is **off by default**. | https://aider.chat/docs/config/options.html |

No harness writes raw reasoning. Codex writes `content: null` beside an encrypted blob; Claude Code a summary beside a signature. Everything past that is inference.

## 2. What the closed APIs add beyond the transcript

| Provider | Token logprobs | Full reasoning | Exact server-side prompt | Source |
| --- | --- | --- | --- | --- |
| Anthropic Messages API | **No.** No `logprobs`/`top_logprobs` parameter exists. | **No.** "No `display` setting returns the raw chain of thought." `redacted_thinking.data` and `signature` are encrypted. You are billed for full thinking tokens you never see. | No. Server reconstructs thinking by decrypting `signature`. | https://platform.claude.com/docs/en/build-with-claude/thinking |
| OpenAI Responses API | Yes for text `logprobs`/`top_logprobs` (0–5); **not for reasoning tokens.** | **No.** "reasoning tokens are not visible via the API... billed as output tokens." Only `summary` (`auto`/`concise`/`detailed`) and `encrypted_content`. | No. | https://developers.openai.com/api/docs/guides/reasoning |
| Google Gemini | Not exposed for reasoning. | **No.** `thinking_summaries: auto` gives summaries; thought signatures are "encrypted representations"; docs say raw traces are not exposed. Accounting via `total_thought_tokens`. | No. | https://ai.google.dev/gemini-api/docs/thinking |

So: **from all three, a third party gets none of token logprobs, full reasoning, or the exact prompt.** You get token counts and a summary written by a different model.

## 3. What local open-weight serving gives you

| Stack | What is exposed | Source |
| --- | --- | --- |
| vLLM | `logprobs` (N per output token, **`-1` returns all `vocab_size`**), `prompt_logprobs` (per prompt token, also `-1` for full vocab), `logprob_token_ids` for a targeted subset. | https://docs.vllm.ai/en/latest/api/vllm/sampling_params.html |
| llama.cpp `llama-server` | `n_probs` (top-N per generated token), `top_logprobs`, `return_tokens` for raw token ids, `post_sampling_probs`. OpenAI-compatible `/v1/chat/completions`. | https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md |
| Ollama | `logprobs` + `top_logprobs` (0–20) on both native and OpenAI-compatible APIs since v0.12.11. | https://github.com/ollama/ollama/releases/tag/v0.12.11 |
| SGLang | `return_logprob`, `logprob_start_len` (0 for prompt logprobs), `top_logprobs_num`, and **`return_hidden_states`** — activations without patching the server. | https://docs.sglang.io/basic_usage/sampling_params.html |
| Reasoning models | DeepSeek-R1 emits `<think>…</think>`; Qwen3 gates it on `enable_thinking`; vLLM and SGLang ship `--reasoning-parser` for both. The **actual** chain of thought, not a summary. | https://huggingface.co/Qwen/Qwen3-8B |

And you write the prompt, so you know it byte for byte.

Harnesses that point at a local OpenAI-compatible endpoint:

| Harness | How | Source |
| --- | --- | --- |
| Aider | `OPENAI_API_BASE=<endpoint>`, then `aider --model openai/<name>` | https://aider.chat/docs/llms/openai-compat.html |
| Codex CLI | `[model_providers.x]` with `base_url`, `wire_api`, `env_key`. `wire_api = "responses"` is the only supported value — a chat-completions server needs a shim. | https://learn.chatgpt.com/docs/config-file/config-reference |
| Cline | OpenAI Compatible provider, base URL + model id | https://docs.cline.bot/provider-config/openai-compatible |
| OpenCode | provider block, `@ai-sdk/openai-compatible`, `options.baseURL` | https://opencode.ai/docs/providers/ |
| Claude Code | `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN`, documented for LLM gateways. Needs an Anthropic-shaped proxy, not an OpenAI one. | https://code.claude.com/docs/en/llm-gateway-connect |

All five keep writing their normal transcripts, so these sessions land in the index unchanged — the local endpoint adds a second, richer stream.

## 4. Prior art

| Work | What it measures | AgentWorth overlap | Gap AgentWorth could fill |
| --- | --- | --- | --- |
| ATIF v1.8 (Harbor) | JSON trajectory interchange: Step / ToolCall / ObservationResult, per-step token metrics, subagent trajectories. Used by NeMo Relay, OpenHands. | AgentWorth already ships `crates/export-atif`. | Nothing writes ATIF *from real developer sessions across 20 harnesses*. Nobody has that dataset. | https://www.harborframework.com/docs/agents/trajectory-format |
| SWE-agent `.traj`, SWE-bench experiments | (thought, action, observation) per step on benchmark tasks. | Same shape, different provenance. | Benchmarks have ground truth; AgentWorth has real repos without it. Complementary. | https://github.com/SWE-agent/SWE-agent/blob/main/docs/usage/trajectories.md |
| OTel GenAI semantic conventions | `gen_ai.operation.name`, `invoke_agent`, `execute_tool` spans; content as events. Mostly experimental. | AgentWorth's event model is a private schema. | OTel-shaped spans would make AgentWorth readable by every existing pipeline for free. | https://github.com/open-telemetry/semantic-conventions-genai |
| Langfuse / LangSmith / AgentOps | Live instrumentation: traces, spans, cost, latency, judge scores. All need the app instrumented first. | Cost and token accounting. | They cannot see an uninstrumented session. AgentWorth is retrospective and needs no harness cooperation. | https://langfuse.com/docs/observability/overview |
| MAST, "Why Do Multi-Agent LLM Systems Fail?" | 14 failure modes, 3 categories, 1600+ annotated traces, 7 frameworks; κ=0.88. | AgentWorth's loop and recovery detectors are ad-hoc versions of a few MAST modes. | MAST's labels would give a *recognised* taxonomy instead of bespoke flags. | https://arxiv.org/abs/2503.13657 |
| TRAIL | 148 annotated traces, 841 errors. Best model scores **11%** at finding them. | — | The 11% is the argument for AgentWorth's thesis: don't ask a model to judge a trace, read structured evidence off it. | https://arxiv.org/abs/2505.08638 |
| "From Confident Closing to Silent Failure" | 9,876 tau2-bench + 1,879 AppWorld trajectories. **75.8% false-success among self-assessing coding-agent trajectories with explicit status claims.** LLM judges reach 0.65 / 0.54 AUROC; TF-IDF reaches 0.83–0.95. | AgentWorth's evidence ladder, independently validated. | Same measurement on real work, not benchmarks. Strongest external support the ladder has. | https://arxiv.org/abs/2606.09863 |
| Chroma, "Context Rot" | 18 frontier models degrade with input length; shuffled haystacks beat coherent ones. | `crates/scoring/src/context_rot.rs` already scores within-session degradation. | Chroma measured synthetic tasks. AgentWorth can measure it against *verified outcomes* in real repos. | https://www.trychroma.com/research/context-rot |
| Prompt-caching economics | 5m writes 1.25×, 1h writes 2×, reads 0.1× base input; fields are in every transcript. | AgentWorth sums `cache_read_input_tokens`. | Nobody has published the per-session fixed-context tax on real sessions. AgentWorth can. | https://platform.claude.com/docs/en/build-with-claude/prompt-caching |

## 5. Where the research edge is

| Question | Answerable from transcripts alone? | Notes |
| --- | --- | --- |
| (a) Asserts facts without checking, how often | **Yes**, and the best question here. A claim with no preceding tool call that could have grounded it is structural, not a judgement. Extends arXiv:2606.09863 to real repos. | https://arxiv.org/abs/2606.09863 |
| (b) Confidence vs correctness | **No — needs local models.** Hedging text is a weak proxy. Calibration needs logprobs: no closed API gives them, every local stack does. | https://docs.vllm.ai/en/latest/api/vllm/sampling_params.html |
| (c) What survives compaction; does re-injecting change outcomes | **Descriptively yes** — `compactMetadata` gives it free. The *causal* half needs a harness you control that re-injects. | measured |
| (d) Repo vs model on verified-outcome rate | **Correlation yes, causation no.** Confounded: you pick the model based on the task. Needs randomised assignment within a repo, which the local stack makes cheap. | — |
| (e) Fixed context cost per session | **Yes, today.** `cache_creation_input_tokens` on a session's first request is system prompt + tools + CLAUDE.md, at 1.25× input. Unpublished; lowest-effort real result here. | https://platform.claude.com/docs/en/build-with-claude/prompt-caching |
| (f) Do repeated corrections land at source | **Yes.** A repeated correction, then a later `Edit` on `CLAUDE.md`/`AGENTS.md` — both already typed events. Pure transcript work. | measured |
| Attention, circuits, why it went wrong | **Not from outside a lab** for closed models. SGLang's `return_hidden_states` covers open weights only. | https://docs.sglang.io/basic_usage/sampling_params.html |

Do (a), (e) and (f) first — no new infrastructure, and (e) is a publishable number now.

## 6. Proposed experiment: the open-model instrument

**Hypothesis.** Verbalised confidence in a completion claim does not predict whether it survives verification; mean token logprob over the claim sentence does.

| Choice | What | Why | Source |
| --- | --- | --- | --- |
| Model | Qwen3-8B, thinking on; Qwen3-32B if the Lenovo has the VRAM | Real `<think>` CoT, serves locally, documented reasoning parser | https://huggingface.co/Qwen/Qwen3-8B |
| Serving | SGLang on the Lenovo, OpenAI-compatible endpoint over Tailscale | Only stack giving logprobs *and* `return_hidden_states` in one server | https://docs.sglang.io/basic_usage/sampling_params.html |
| Harness | Aider: `OPENAI_API_BASE`, `--model openai/…`, `--llm-history-file` on | One env var, and the only harness that writes the exact prompt to disk | https://aider.chat/docs/llms/openai-compat.html |
| Adapter records | Per response: rendered prompt, output token ids, per-token logprob + top-5, the `<think>` block, outcome rung | The first four are unobtainable from any closed API | — |
| Task set | 60–100 tasks from one fixed repo, scored by the ladder plus git/test exit codes | Constant repo removes the (d) confound | — |

**Run.** Mac drives Aider and holds the SQLite index; Lenovo serves the model. One task per run — nothing heavy on the Mac.

**Falsified if** mean logprob over the claim span separates verified from unverified outcomes no better than chance (AUROC ≤0.55), or no better than the text-only hedging baseline. TF-IDF alone reaches 0.83–0.95 on this distinction (arXiv:2606.09863), so failing to beat 0.55 is a real negative result worth publishing.

**Cheaper second run, same rig:** ablate compaction — same tasks, `preservedSegment` re-injection on and off. Falsified if the verified-outcome rate is unchanged.
