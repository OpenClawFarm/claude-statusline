# claude-statusline

A HUD-style status line for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) that turns your prompt bar into a real-time dashboard — model, context, network health, throughput, latency, quotas, and cost, all in one line.

```
📂 ~/myproject  main~+ │ Opus 4.6 ◕high 280k 🟢 55 tps 297ms │ 5h ██░░░░ 32% 3h42m  7d █░░░░░ 15% 5d  $0.3
```

> **One glance. Everything you need to know.**

---

## Why This Exists

Claude Code gives you a powerful AI pair programmer, but the default status line is... minimal. When you're deep in an Opus session burning through a Max subscription, you want to know:

- Am I about to hit the context limit?
- Is my network connection degrading?
- How fast is the API actually responding?
- How much quota have I burned? When does it reset?
- What's this session costing me?

This script answers all of that **without leaving your terminal**.

---

## Features

### 10 Modules, One Line

| Module | Display | Source |
|--------|---------|--------|
| **Directory** | `📂 ~/path` clickable | CC JSON |
| **Git Branch** | ` main~+` clickable | `git` |
| **Model** | `Opus 4.6` | CC JSON |
| **Effort** | `◔` `◑` `◕` clickable | settings.json |
| **Context** | `280k` color-coded | CC JSON |
| **Network** | 🟢🟡🔴 + error type | JSONL |
| **TPS** | `55 tps` median throughput | JSONL |
| **RTT** | `297ms` median latency | ping |
| **Quotas** | `5h ██░░░░ 32%` bars | CC JSON |
| **Cost** | `$0.3` / `$10` | CC JSON |

### Network Health Monitor

Inspired by [claudebubble](https://github.com/limin112/claudebubble), but built directly into the status line. No floating window, no extra process.

The monitor reads the tail of your Claude Code session's JSONL transcript and uses **positional comparison** to determine if the session is actively retrying:

```
tail -100 session.jsonl
    ├── last "retryInMs" at line 87   ← last network error
    ├── last "stop_reason" at line 52  ← last successful response
    └── 87 > 52 → currently retrying → 🟡 or 🔴
```

| Indicator | Meaning | Action |
|-----------|---------|--------|
| 🟢 | All clear | — |
| 🟡`3rst` | 1-4 retries, ECONNRESET | Switch proxy node |
| 🟡`2cert` | 1-4 retries, TLS/cert error | Check DNS & SSL config |
| 🔴`8` | 5+ retries | Serious connectivity issue |
| 🔴`6 504` | 5+ retries, gateway timeout | Check CDN proxy settings |

**Auto-recovery**: returns to 🟢 the instant a successful response arrives.

**Error tags** tell you *what to fix*:
- `rst` — Connection reset. Proxy overloaded or ISP interference. **Switch nodes.**
- `cert` — TLS certificate verification failed. DNS resolving to wrong IP. **Fix DNS/SSL.**
- `504` — Cloudflare proxy timeout on long Opus outputs. **Use DNS-only mode.**

### TPS & RTT — Dual Sliding Window Median

Both metrics use **sliding window median** to smooth out noise while still tracking real changes:

```
🟢 55 tps 297ms
    │       │
    │       └── RTT: median of last 5 ping rounds (3 ICMP packets each)
    └────────── TPS: median of last 5 responses (≥100 tokens, 10-500 tps)
```

**TPS** (tokens per second) is calculated from the JSONL session transcript:

```
TPS = output_tokens / streaming_time
```

Three filters ensure accuracy:
- **≥100 output tokens** — excludes thinking-heavy short responses
- **≥0.3s duration** — excludes timing-imprecise sub-second bursts
- **10-500 tps range** — excludes anomalous outliers (e.g., 833k tps)

The time is measured from the *preceding JSONL entry* to the assistant response, which automatically excludes tool execution time in multi-tool turns.

Normal ranges (through proxy):

| Model | TPS |
|-------|-----|
| Opus 4.6 | 35-55 |
| Sonnet 4.6 | 80-120 |
| Haiku 4.5 | 150-200 |

**RTT** (round-trip time) pings `api.anthropic.com` every 30 seconds:
- Each round sends 3 ICMP packets → takes the median
- Appends to a 5-round sliding window → takes the median of the window
- Falls back to `curl` TTFB if ICMP is blocked

| RTT | Color | Meaning |
|-----|-------|---------|
| <300ms | green | Normal |
| 300-499ms | yellow | Degraded |
| 500ms+ | red | High latency |

A sudden TPS drop or RTT spike signals network degradation *before* you hit full retry mode — the early warning that complements the 🟢🟡🔴 indicators.

### Clickable Everything (OSC 8)

Hold **Cmd** (macOS) or **Ctrl** (Linux) and click any highlighted element:

| Element | Click → | Shortcut |
|---------|---------|----------|
| `📂 ~/path` | Finder | Cmd+Click |
| ` main` | GitHub branch page | Cmd+Click |
| `◕high` | CycleEffort.app | Cmd+Click |

Requires a terminal that supports [OSC 8 hyperlinks](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda): iTerm2, Kitty, WezTerm, Ghostty.

### Color System

Three-tier visual hierarchy inspired by [Starship](https://starship.rs), [Lazygit](https://github.com/jesseduffield/lazygit), and [btop](https://github.com/aristocratos/btop):

```
 Bold Bright          Normal             Dim
┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│ Directory    │  │ Model name   │  │ Separators   │
│ Git branch   │  │ Progress bar │  │ Labels       │
│ Key values   │  │ Cost         │  │ Error tags   │
└──────────────┘  └──────────────┘  └──────────────┘
```

Quota bars: blue (<75%) → magenta (75-89%) → red (90%+).
Context window: green (<70%) → yellow (70-84%) → red (85%+).
Cost: `<$1` shows one decimal (`$0.3`), `≥$1` rounds to integer (`$10`).

---

## Installation

### Quick Start (One Command)

```bash
curl -o ~/.claude/statusline-command.sh \
  https://raw.githubusercontent.com/OpenClawFarm/claude-statusline/main/claude-statusline.sh
```

Then add to `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "bash ~/.claude/statusline-command.sh"
  }
}
```

Restart Claude Code (or wait — settings hot-reload).

### Windows (Git Bash)

Works out of the box. Same install — just make sure `jq` and `python3` are on your PATH:

```bash
# Install jq: https://jqlang.github.io/jq/download/
# Install Python: https://www.python.org/downloads/
curl -o ~/.claude/statusline-command.sh \
  https://raw.githubusercontent.com/OpenClawFarm/claude-statusline/main/claude-statusline.sh
```

OSC 8 clickable links work in Windows Terminal.

### Requirements

| Dependency | macOS | Windows (Git Bash) |
|------------|-------|--------------------|
| Claude Code v1.0.71+ | Required | Required |
| jq | `brew install jq` | [Download](https://jqlang.github.io/jq/download/) |
| Python 3.9+ | Pre-installed | [Download](https://www.python.org/downloads/) |
| Git | Pre-installed | Included in Git Bash |

---

## How It Works

The script receives JSON from Claude Code on stdin every ~1 second:

```json
{
  "model": { "display_name": "Claude Opus 4.6" },
  "workspace": { "current_dir": "~/project" },
  "context_window": { "remaining_percentage": 72 },
  "rate_limits": {
    "five_hour": { "used_percentage": 32, "resets_at": 1743500000 },
    "seven_day": { "used_percentage": 15, "resets_at": 1743800000 }
  },
  "cost": { "total_cost_usd": 1.5 }
}
```

**Modules 1-5, 9, 10** parse this JSON with `jq`.

**Module 6 (Network)** and **Module 7 (TPS)** read the session's JSONL transcript at `~/.claude/projects/`. The script auto-discovers the active session across all project directories using `find -mmin -5`, so it works even after `cd` or `/add-dir`.

**Module 8 (RTT)** pings `api.anthropic.com` every 30 seconds and caches the result.

### Performance Budget

| Operation | Time | Frequency |
|-----------|------|-----------|
| jq parse (x6 calls) | ~20ms | every refresh |
| git queries (x4, gc.auto=0) | ~15ms | every refresh |
| tail + grep (network check) | ~5ms | every refresh |
| python3 TPS calculation | ~30ms | every refresh |
| ping (3 packets) | ~300ms | every 30s |
| **Per refresh** | **~70ms** | |

The status line refreshes every ~1 second. 70ms is imperceptible. The 300ms ping runs only once per 30 seconds and reads from cache otherwise.

---

## Customization

The script is a single bash file — edit directly. Common tweaks:

### TPS Filtering

```bash
# Minimum tokens for TPS sample (default: 100)
if ot >= 100:

# TPS range filter (default: 10-500)
if 10 <= tps <= 500: samples.append(tps)

# Sliding window size (default: last 5)
recent = samples[-5:]
```

### RTT Ping Interval

```bash
# Seconds between pings (default: 30)
if [ "$rtt_age" -gt 30 ]; then
```

### Context Window Size

```bash
case "$model" in
    *Opus*|*opus*) ctx_total=1000 ;;   # 1M context
    *)             ctx_total=200 ;;     # 200k context
esac
```

### Disable Modules

Comment out any `*_part` variable and remove from the assembly line at the bottom.

---

## Terminal Compatibility

| Terminal | Full Support | Notes |
|----------|:---:|-------|
| iTerm2 | Yes | All features including OSC 8 |
| Kitty | Yes | |
| WezTerm | Yes | |
| Ghostty | Yes | |
| Windows Terminal | Yes | Colors + OSC 8 |
| VS Code Terminal | Partial | Colors work, OSC 8 may not |
| macOS Terminal.app | Partial | Colors work, no OSC 8 |
| tmux | Partial | OSC 8 needs `allow-passthrough` |

---

## Acknowledgments

- [claudebubble](https://github.com/limin112/claudebubble) by [@limin112](https://github.com/limin112) — the floating network monitor that inspired our JSONL-based health detection
- [Starship](https://starship.rs) — color conventions (cyan directories, magenta git)
- [Lazygit](https://github.com/jesseduffield/lazygit) — branch color inspiration
- [btop](https://github.com/aristocratos/btop) — HUD-style progress bar aesthetics
- Built for the [OpenClawFarm](https://github.com/OpenClawFarm) infrastructure

---

## License

[MIT](LICENSE)
