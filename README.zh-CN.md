# AgentWorth (简体中文)

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

[![npm](https://img.shields.io/npm/v/agentworth?style=flat-square&color=000000)](https://www.npmjs.com/package/agentworth)
[![License](https://img.shields.io/badge/license-Apache--2.0-000000?style=flat-square)](LICENSE)
[![Privacy](https://img.shields.io/badge/telemetry-zero%20(100%25%20local)-000000?style=flat-square)](#隐私与本地优先保证)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-000000?style=flat-square)](#快速开始)
[![Website](https://img.shields.io/badge/website-agentworth.dev-000000?style=flat-square)](https://agentworth.dev)

**你的智能体留下了账单。**  
看清 AI 编程智能体在你的机器上实际执行了什么操作。

AgentWorth 是一款采用本地优先架构的 Rust 原生工具，专门用于自动发现、规范化并索引存放在本机隐藏目录（dotfiles）中的 AI 智能体历史轨迹。

它将数 GB 晦涩难读的 JSONL 日志转换为清晰的度量指标、执行轨迹与可验证成果——精确展示你消耗的令牌总数、哪些任务成功执行、哪些错误修复陷入死循环，以及每行代码究竟由哪个 AI 智能体编写。

```text
┌─────────────────────────────────────────────────────────────┐
│                      * * * 账 单 (RECEIPT) * * *            │
│ 消耗令牌总量 ................................... 77,920,000 │
│ 预估 API 支出 .................................. $218.40    │
│ 已索引会话数 ................................... 695        │
│ 识别到的智能体 ................................. 11 种      │
│ 验证成果率 (Verified Outcomes) ................. 412 (59%)  │
│ 主要适配器 ..................................... Claude     │
└─────────────────────────────────────────────────────────────┘
```

---

## 快速开始

| 安装方式 | 执行命令 | 说明 |
| :--- | :--- | :--- |
| **一键安装脚本** | `curl -fsSL https://agentworth.dev/install.sh \| sh` | 直接将预编译原生二进制文件安装至 `~/.local/bin`。 |
| **Cargo (Rust 原生)** | `cargo install agentworth-cli` | 编译并安装 `agentworth` 与简称 `archie` 至 `~/.cargo/bin`。 |
| **NPX (免安装即用)** | `npx agentworth stats` | 无需手动安装即可快速执行。 |

```bash
# 1. 扫描并索引本机所有智能体历史日志
archie scan

# 2. 查看全局令牌消耗与模型分布统计
archie stats

# 3. 审计每日支出与 5 小时滑动窗口令牌消耗速率
archie stats usage --period day
archie window show

# 4. 按代码行追溯 AI 会话归属 (AI Blame)
archie repo blame src/main.rs

# 5. 启动本地交互式账单浏览器前端
archie serve --open
```

> **提示：** 所有命令都可以使用简称 `archie`（例如：`archie stats`、`archie repo blame`）。旧的 `agwt` 仍然可用，但不再出现在文档里。

---

## 已支持的 20 种智能体适配器

AgentWorth 能够完全离线、自动流式解析本机上的 20 种主流智能体日志：

1. **Claude Code** (`~/.claude/projects/`, `~/.claude/sessions/`)
2. **Google Antigravity / Gemini CLI** (`~/.gemini/antigravity-cli/`)
3. 🐋 **DeepSeek Code** (`~/.deepseek/`, `~/.deepseek-coder/` - 原生解析 R1/V3 推理思考 Token)
4. 🌙 **Kimi Code (月之暗面)** (`~/.kimi-code/`, `~/.kimi/sessions/wire.jsonl` - Wire 协议与子智能体)
5. ⚡ **MiniMax** (`~/.minimax/`, `~/.minimax-agent/` - 编码规划与轨迹)
6. 🐉 **Qwen Code / 通义千问 (阿里)** (`~/.qwen/`, `~/.qwen-agent/` - Qwen 2.5 Coder 轨迹)
7. 🧠 **智谱 CodeGeeX / GLM-4** (`~/.codegeex/`, `~/.zhipu/` - IDE 插件与 CLI 历史)
8. 🛠️ **Aider** (`.aider.chat.history.md`, `~/.aider/` - Git 提交与 Diff 轨迹)
9. 👁️ **Cline & Roo-Code** (VSCode `globalStorage/saoudrizwan.claude-dev/tasks/`, `roo-cline/`)
10. 🌊 **Windsurf / Cascade** (`~/.codeium/windsurf/`, `~/.windsurf/` - 级联执行缓存)
11. 🦾 **Manus** (`~/.manus/` - 自主浏览器与编码轨迹)
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

## 隐私与本地优先保证

- **100% 离线运行**：零云端遥测，绝不私自上传任何用户数据。
- **无损原始日志**：不对原始转录日志进行任何篡改或冗余复制。
- **零知识隐私脱敏**：导出为标准 ATIF 格式时自动遮蔽 API Key、凭证及私有路径。

---

## 开源协议

Apache License 2.0.
