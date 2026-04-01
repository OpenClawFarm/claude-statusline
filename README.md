# claude-statusline

A real-time HUD for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) — model, context, network, throughput, latency, quotas, and cost in one status line.

```
📂 ~/project  main~+ │ Opus 4.6 ◕high 280k 🟢 55 tps 173ms │ 5h ██░░░░ 32% 3h42m  7d █░░░░░ 15% 5d  $0.3
```

## Modules

| # | Module | Display | Source |
|---|--------|---------|--------|
| 1 | Directory | `📂 ~/path` | CC JSON |
| 2 | Git branch | ` main~+` | git |
| 3 | Model | `Opus 4.6` | CC JSON |
| 4 | Effort | `◔` `◑` `◕` | settings.json |
| 5 | Context | `280k` | CC JSON |
| 6 | Network | 🟢🟡🔴 | JSONL |
| 7 | TPS | `55 tps` | JSONL |
| 8 | RTT | `173ms` | ping |
| 9 | Quotas | `5h ██░░░░ 32%` | CC JSON |
| 10 | Cost | `$0.3` | CC JSON |

Directory, git branch, and effort level are clickable via [OSC 8](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda) (iTerm2, Kitty, WezTerm, Ghostty, Windows Terminal).

## Install

```bash
curl -o ~/.claude/statusline-command.sh \
  https://raw.githubusercontent.com/OpenClawFarm/claude-statusline/main/claude-statusline.sh
```

Add to `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "bash ~/.claude/statusline-command.sh"
  }
}
```

Restart Claude Code. Requires **jq** and **Python 3.9+**.

<details>
<summary>Windows (Git Bash)</summary>

```powershell
winget install jqlang.jq
winget install Python.Python.3.13
```

The script auto-detects Windows and adjusts `stat`, `ping`, cache paths, and Python/jq lookup. No manual patching needed.

</details>

## How It Works

Claude Code pipes JSON to the script every ~1s. Modules 1–5, 9, 10 parse it with `jq`. The remaining modules work differently:

**Network (6)** — Reads the JSONL session transcript using positional comparison: if the last `retryInMs` appears after the last `stop_reason`, the session is retrying. After recovery, recent retry count is retained (e.g. `🟢3`) so transient issues are visible. Error tags (`rst`, `cert`, `504`) indicate what to fix. Auto-discovers active sessions across all project directories. Inspired by [claudebubble](https://github.com/limin112/claudebubble).

**TPS (7)** — Calculates `output_tokens / streaming_time` from JSONL, excluding tool execution time. Filters: ≥100 tokens (≥10 during cold start), ≥0.3s, 10–500 tps range. Sliding window median over last 3 samples. Aggregates across all active sessions.

**RTT (8)** — Pings `api.anthropic.com` every 5s (single ICMP packet, 2s timeout). Sliding window median over last 3 rounds. Falls back to `curl` TTFB if ICMP is blocked.

## Color Coding

| Metric | Green | Yellow | Red |
|--------|-------|--------|-----|
| Context | <70% used | 70–84% | 85%+ |
| Quota bars | <75% used | 75–89% | 90%+ |
| RTT | <300ms | 300–499ms | 500ms+ |
| Network | 🟢 clear / 🟢*n* recovered | 🟡 1–4 retries | 🔴 5+ retries |

## Compatibility

| Terminal | Colors | OSC 8 Links |
|----------|:------:|:-----------:|
| iTerm2 / Kitty / WezTerm / Ghostty | Yes | Yes |
| Windows Terminal | Yes | Yes |
| VS Code Terminal | Yes | Partial |
| macOS Terminal.app | Yes | No |
| tmux | Yes | Needs `allow-passthrough` |

Works on macOS, Linux, and Windows (Git Bash).

## License

[MIT](LICENSE)

## Acknowledgments

- [claudebubble](https://github.com/limin112/claudebubble) — network health detection inspiration
- [Starship](https://starship.rs) / [btop](https://github.com/aristocratos/btop) — color and HUD conventions
