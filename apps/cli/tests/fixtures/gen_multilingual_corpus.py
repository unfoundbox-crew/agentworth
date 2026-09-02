#!/usr/bin/env python3
"""Generate a synthetic multi-adapter, multi-byte-heavy session corpus.

This is the fixture behind the CI end-to-end gate for the `recovery.rs:571` panic
(`agwt scan` crashing on a Hebrew byte-index slice, see CHANGELOG 0.1.15). It writes
Claude Code, Codex, and opencode-shaped session files whose text fields are packed with
Hebrew, Arabic, CJK, emoji, and a combining mark, at lengths chosen to straddle the old
fixed byte-index cut points (79, 80, 81, 120, 200), plus a couple of multi-MB files so a
truncation bug on a large transcript surfaces here too, before it reaches a real machine.

Usage: gen_multilingual_corpus.py <output_dir>
"""
import json
import os
import sys

HEBREW = "ך"  # 2 bytes/char
ARABIC = "خطأ في تشغيل الاختبار رجاء أعد المحاولة "  # 2 bytes/char
CJK = "测试失败了请修复这个问题"  # 3 bytes/char
EMOJI = "🎉🚀🔥"  # 4 bytes/char
COMBINING = "éf́ǵ"  # combining acute accents

def multiscript(n_repeats: int) -> str:
    """A string mixing every script, long enough to straddle byte offsets 79-81, 120, 200
    many times over regardless of where in it a fixed byte-index cut lands."""
    unit = HEBREW + ARABIC + CJK + EMOJI + COMBINING
    return unit * n_repeats


def write_jsonl(path: str, lines: list[str]) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        for line in lines:
            f.write(line)
            f.write("\n")


def claude_code_line(obj: dict) -> str:
    return json.dumps(obj, ensure_ascii=False)


def gen_claude_code(root: str) -> None:
    proj = os.path.join(root, ".claude", "projects", "multilingual-corpus")

    # Session 1: the exact crash shape -- "panic: " (7 ASCII bytes) + Hebrew puts byte
    # offset 80 mid-character (7 + 73 = one byte into the 37th 2-byte char).
    hebrew_panic = "panic: " + HEBREW * 150
    write_jsonl(
        os.path.join(proj, "session_hebrew_panic.jsonl"),
        [
            claude_code_line({"type": "user", "timestamp": "2026-08-29T10:00:00Z", "content": "תקן את הבדיקה שנכשלה"}),
            claude_code_line({
                "type": "assistant", "timestamp": "2026-08-29T10:00:02Z",
                "model": "claude-3-5-sonnet-20241022",
                "usage": {"input_tokens": 5000, "output_tokens": 800, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
                "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "cargo test"}}],
            }),
            claude_code_line({"type": "tool_result", "timestamp": "2026-08-29T10:00:04Z", "tool_use_id": "t1", "content": hebrew_panic, "is_error": False}),
            claude_code_line({
                "type": "assistant", "timestamp": "2026-08-29T10:00:06Z",
                "model": "claude-3-5-sonnet-20241022",
                "usage": {"input_tokens": 200, "output_tokens": 30, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
                "content": [{"type": "tool_use", "id": "t2", "name": "Edit", "input": {"file_path": "src/lib.rs", "old_string": "a", "new_string": "b"}}],
            }),
            claude_code_line({"type": "tool_result", "timestamp": "2026-08-29T10:00:07Z", "tool_use_id": "t2", "content": "File modified successfully", "is_error": False}),
            claude_code_line({
                "type": "assistant", "timestamp": "2026-08-29T10:00:08Z",
                "model": "claude-3-5-sonnet-20241022",
                "usage": {"input_tokens": 200, "output_tokens": 30, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
                "content": [{"type": "tool_use", "id": "t3", "name": "Bash", "input": {"command": "cargo test"}}],
            }),
            claude_code_line({"type": "tool_result", "timestamp": "2026-08-29T10:00:10Z", "tool_use_id": "t3", "content": "test result: ok. 8 passed; 0 failed", "is_error": False}),
        ],
    )

    # Session 2: every script mixed, at lengths straddling every old cut point (79, 80,
    # 81, 120, 200 bytes) many times over.
    write_jsonl(
        os.path.join(proj, "session_multiscript.jsonl"),
        [
            claude_code_line({"type": "user", "timestamp": "2026-08-29T11:00:00Z", "content": multiscript(3)}),
            claude_code_line({
                "type": "assistant", "timestamp": "2026-08-29T11:00:02Z",
                "model": "claude-3-5-haiku-20241022",
                "usage": {"input_tokens": 900, "output_tokens": 250, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
                "content": [{"type": "text", "text": "Error: " + multiscript(5)}],
            }),
        ],
    )

    # Session 3: a multi-MB transcript (~2.5MB), so a truncation bug on large multi-byte
    # content surfaces in CI, not on a real 8,000-source machine.
    big_output = "test result: FAILED. " + multiscript(20000)  # well over 2MB
    write_jsonl(
        os.path.join(proj, "session_large_multibyte.jsonl"),
        [
            claude_code_line({"type": "user", "timestamp": "2026-08-29T12:00:00Z", "content": "run the full suite"}),
            claude_code_line({
                "type": "assistant", "timestamp": "2026-08-29T12:00:02Z",
                "model": "claude-3-5-sonnet-20241022",
                "usage": {"input_tokens": 5000, "output_tokens": 800, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
                "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "cargo test --workspace"}}],
            }),
            claude_code_line({"type": "tool_result", "timestamp": "2026-08-29T12:00:30Z", "tool_use_id": "t1", "content": big_output, "is_error": False}),
            claude_code_line({
                "type": "assistant", "timestamp": "2026-08-29T12:00:32Z",
                "model": "claude-3-5-sonnet-20241022",
                "usage": {"input_tokens": 200, "output_tokens": 30, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
                "content": [{"type": "text", "text": "That is a lot of failures: " + multiscript(4)}],
            }),
        ],
    )


def gen_codex(root: str) -> None:
    sessions = os.path.join(root, ".codex", "sessions")

    write_jsonl(
        os.path.join(sessions, "rollout-2026-09-02T00-00-00-019eed64-07e0-7ad0-a4bd-3ec244120cd1.jsonl"),
        [
            json.dumps({"role": "user", "content": "أصلح هذا الخطأ من فضلك"}, ensure_ascii=False),
            json.dumps({"role": "assistant", "content": "سأقوم بإصلاح ذلك الآن. " + multiscript(4)}, ensure_ascii=False),
            json.dumps({"role": "tool", "content": "panic: " + HEBREW * 60}, ensure_ascii=False),
            json.dumps({"role": "assistant", "content": "تم الإصلاح. " + CJK * 30}, ensure_ascii=False),
        ],
    )

    # A multi-MB Codex session.
    write_jsonl(
        os.path.join(sessions, "rollout-2026-09-02T00-05-00-019eed64-07e0-7ad0-a4bd-3ec244120cd2.jsonl"),
        [
            json.dumps({"role": "user", "content": "run everything"}, ensure_ascii=False),
            json.dumps({"role": "assistant", "content": multiscript(15000)}, ensure_ascii=False),  # ~1.7MB
        ],
    )


def gen_opencode(root: str) -> None:
    # `is_candidate_opencode_file` requires the full path string to contain "opencode"
    # (case-insensitive), matching OpenCode's own legacy file-store layout:
    # `.../opencode/project/<mangled-slug>/<session>.jsonl`.
    proj = os.path.join(root, "opencode", "project", "-tmp-multilingual-corpus")

    write_jsonl(
        os.path.join(proj, "session1.jsonl"),
        [
            json.dumps({"type": "user_message", "timestamp": "2026-08-29T10:00:00Z", "content": "测试修复请求 " + CJK * 5}, ensure_ascii=False),
            json.dumps({
                "type": "assistant_message", "timestamp": "2026-08-29T10:00:04Z", "model": "deepseek-coder-v2",
                "usage": {"input_tokens": 350, "output_tokens": 110, "cache_read_tokens": 50},
                "thinking": "Checking logs... " + ARABIC, "content": "I will inspect the query logs.",
            }, ensure_ascii=False),
            json.dumps({"type": "tool_call", "timestamp": "2026-08-29T10:00:06Z", "id": "tc_1", "name": "bash", "arguments": {"command": "cargo test"}}, ensure_ascii=False),
            json.dumps({"type": "tool_result", "timestamp": "2026-08-29T10:00:08Z", "tool_call_id": "tc_1", "output": "panic: " + HEBREW * 90 + EMOJI, "is_error": False}, ensure_ascii=False),
            json.dumps({"type": "assistant_message", "timestamp": "2026-08-29T10:00:10Z", "content": "Fixed. " + COMBINING * 10}, ensure_ascii=False),
        ],
    )

    # A multi-MB opencode session.
    write_jsonl(
        os.path.join(proj, "session_large.jsonl"),
        [
            json.dumps({"type": "user_message", "timestamp": "2026-08-29T11:00:00Z", "content": "run everything"}, ensure_ascii=False),
            json.dumps({"type": "assistant_message", "timestamp": "2026-08-29T11:00:04Z", "content": multiscript(15000)}, ensure_ascii=False),  # ~1.7MB
        ],
    )


def main() -> None:
    if len(sys.argv) != 2:
        print("usage: gen_multilingual_corpus.py <output_dir>", file=sys.stderr)
        sys.exit(2)
    root = sys.argv[1]
    os.makedirs(root, exist_ok=True)
    gen_claude_code(root)
    gen_codex(root)
    gen_opencode(root)
    print(f"wrote multilingual corpus to {root}")


if __name__ == "__main__":
    main()
