# AgentWorth (日本語)

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

[![npm](https://img.shields.io/npm/v/agentworth?style=flat-square&color=000000)](https://www.npmjs.com/package/agentworth)
[![License](https://img.shields.io/badge/license-Apache--2.0-000000?style=flat-square)](LICENSE)
[![Privacy](https://img.shields.io/badge/telemetry-zero%20(100%25%20local)-000000?style=flat-square)](#プライバシーとローカルファーストの保証)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-000000?style=flat-square)](#クイックスタート)
[![Website](https://img.shields.io/badge/website-agentworth.dev-000000?style=flat-square)](https://agentworth.dev)

**あなたのエージェントは領収書を残しました。**  
AIコーディングエージェントがあなたのマシンで実際に何をしていたかを可視化します。

AgentWorthは、ローカルのドットファイルに蓄積されたAIエージェントの履歴を自動検出し、正規化してインデックス化する、ローカルファーストのネイティブRustツールです。

ギガバイト単位の読みにくいJSONLログを、明確なメトリクス、実行軌跡（Trajectory）、検証済みアウトカムへと変換します。消費トークン数、タスクの成功率、スタックしたエラーリカバリーループ、各コード行を編集したエージェントの系統を正確に特定します。

```text
┌─────────────────────────────────────────────────────────────┐
│                      * * * 領収書 (RECEIPT) * * *           │
│ 総消費トークン数 ................................ 77,920,000│
│ 推定API利用額 ................................... $218.40   │
│ インデックス済みセッション数 .................... 695       │
│ 検出されたエージェント数 ........................ 11        │
│ 検証済み成果率 (Verified Outcomes) .............. 412 (59%) │
│ 主なエージェント ................................ Claude    │
└─────────────────────────────────────────────────────────────┘
```

---

## クイックスタート

| インストール方法 | コマンド | 説明 |
| :--- | :--- | :--- |
| **単体インストールスクリプト** | `curl -fsSL https://agentworth.dev/install.sh \| sh` | ネイティブバイナリを直接 `~/.local/bin` にインストールします。 |
| **Cargo (Rustネイティブ)** | `cargo install agentworth-cli` | `agentworth` と短縮名 `archie` を `~/.cargo/bin` にビルドしてインストールします。 |
| **NPX (インストール不要)** | `npx -y agentworth@latest stats` | インストールなしで即座に実行できます。 |

```bash
# 1. ローカルのエージェント履歴をスキャンしてインデックスを作成
archie scan

# 2. マシン全体のトークン消費とモデル統計を表示
archie stats

# 3. トークン消費速度と5時間のローリングペーシングを監査
archie stats usage --period day
archie window show

# 4. コード行単位でAIの編集セッションを特定 (AI Blame)
archie repo blame src/main.rs

# 5. ローカルの対話型レシートエクスプローラーUIを起動
archie serve --open
```

> **ヒント:** すべてのコマンドで短縮名 `archie` を使用できます (例: `archie stats`, `archie repo blame`)。以前の `agwt` も動作しますが、ドキュメントには載せていません。

---

## サポートされているエージェント (20種類)

AgentWorthは、ローカル環境に存在する以下の20種類のエージェントログを完全にオフラインで自動ストリーミング解析します：

1. **Claude Code** (`~/.claude/projects/`, `~/.claude/sessions/`)
2. **Google Antigravity / Gemini CLI** (`~/.gemini/antigravity-cli/`)
3. 🐋 **DeepSeek Code** (`~/.deepseek/`, `~/.deepseek-coder/` - R1/V3 思考推論トークン解析)
4. 🌙 **Kimi Code (Moonshot)** (`~/.kimi-code/`, `~/.kimi/sessions/wire.jsonl` - Wireプロトコル・サブエージェント)
5. ⚡ **MiniMax** (`~/.minimax/`, `~/.minimax-agent/` - コーディング計画・軌跡ログ)
6. 🐉 **Qwen Code / 通義千問 (Alibaba)** (`~/.qwen/`, `~/.qwen-agent/` - Qwen 2.5 Coder 軌跡)
7. 🧠 **智譜 CodeGeeX / GLM-4** (`~/.codegeex/`, `~/.zhipu/` - IDE拡張機能・CLI履歴)
8. 🛠️ **Aider** (`.aider.chat.history.md`, `~/.aider/` - Gitコミット・Diff軌跡)
9. 👁️ **Cline & Roo-Code** (VSCode `globalStorage/saoudrizwan.claude-dev/tasks/`, `roo-cline/`)
10. 🌊 **Windsurf / Cascade** (`~/.codeium/windsurf/`, `~/.windsurf/` - 実行キャッシュ解析)
11. 🦾 **Manus** (`~/.manus/` - 自律ブラウザ操作・コーディング軌跡)
12. **OpenAI Codex** (`~/.codex/`)
13. **Cursor Composer** (`~/Library/Application Support/Cursor/` / `~/.config/Cursor/`)
14. **Block Goose** (`~/.local/share/goose/`)
15. **Pi Task Agent** (`~/.pi/agent/`)
16. **Herdr Orchestrator** (`~/.herdr/`)
17. **Nous Hermes** (`~/.hermes/`)
18. **OpenClaw** (`~/.openclaw/`)
19. **xAI Grok CLI** (`~/.grok/`)
20. **OpenCode** (`~/.opencode/`)

---

## プライバシーとローカルファーストの保証

- **100% オフライン動作**: クラウドへのテレメトリ送信や外部アップロードは一切行いません。
- **元ログの非破壊**: 元のトランスクリプトファイルを変更したり複製したりしません。
- **ゼロ知識データ秘匿化 (Zero-Knowledge Redaction)**: ATIFフォーマットでのエクスポート時に、APIキー、トークン、機密パスを自動でマスキングします。

---

## ライセンス

Apache License 2.0.
