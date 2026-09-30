# claude-statusline

A real-time HUD for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) — model, context, network, throughput, latency, and quotas in one status line.

```
📂 ~/project  main~+ │ Opus 4.6 ◕high 280k 🟢 55 tps 173ms │ ⏱ 5h ██░░░░ 32% 3h42m  ☀ 7d █░░░░░ 15%/62% 5d  ✦ Fable ██░░░░ 28%
```

A single native binary (Rust). Claude Code runs the status line about once a second; a render takes ~4 ms plus git (~20 ms inside a repo on macOS), versus ~230 ms for the 2.x bash script it replaces.

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
| 8 | API RTT | `113ms` | HTTPS probe |
| 9 | Quotas | `⏱ 5h ██░░░░ 32%` · `☀ 7d █░░░░░ 15%/62%` | CC JSON |
| 10 | Fable weekly | `Fable ██░░░░ 28%` | OAuth usage API |

Directory, git branch, and effort level are clickable via [OSC 8](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda) (iTerm2, Kitty, WezTerm, Ghostty, Windows Terminal).

## Install

**macOS (Apple Silicon)** — prebuilt binary:

```bash
mkdir -p ~/.claude/bin
curl -fsSL https://github.com/OpenClawFarm/claude-statusline/releases/latest/download/claude-statusline-aarch64-apple-darwin.tar.gz \
  | tar -xz -C ~/.claude/bin
```

Downloading with `curl` leaves no quarantine flag, so it runs as-is. If you fetched the archive with a browser instead, clear it: `xattr -d com.apple.quarantine ~/.claude/bin/claude-statusline`. A `.sha256` file sits next to each release archive.

**Other platforms** — build from source:

```bash
cargo install --git https://github.com/OpenClawFarm/claude-statusline   # → ~/.cargo/bin/claude-statusline
```

Add to `~/.claude/settings.json` (adjust the path for `cargo install`):

```json
{
  "statusLine": {
    "type": "command",
    "command": "~/.claude/bin/claude-statusline"
  }
}
```

Restart Claude Code. Runtime tools: **git** (branch) and **curl** (API RTT, Fable quota). No jq or Python needed.

The 2.x bash script is kept at tag [`v2.3.2`](https://github.com/OpenClawFarm/claude-statusline/tree/v2.3.2).

## How It Works

Claude Code pipes JSON to the binary every ~1s. Modules 1, 3–5 and 9 come straight from it. The rest:

**Git (2)** — Branch plus `~` (working tree differs from HEAD) and `+` (staged changes), from a single `git status --porcelain=v2 --branch -uno`, run with `--no-optional-locks` so it never contends with your own git commands. Outside a work tree git isn't invoked at all. The branch links to the GitHub tree when `origin` is set.

**Network (6)** — Reads the JSONL session transcripts using positional comparison: if the last `retryInMs` appears after the last `stop_reason`, the session is retrying. After recovery, the recent retry count is retained (e.g. `🟢3`) so transient issues stay visible. Error tags (`rst`, `cert`, `504`) indicate what to fix. Active sessions (written in the last 5 minutes, subagents included, newest 10) are discovered across all project directories. Inspired by [claudebubble](https://github.com/limin112/claudebubble).

**TPS (7)** — `output_tokens / streaming_time` from the transcripts, excluding tool execution time. Multi-block responses (thinking/text/tool_use) are grouped by `message.id` and timed from before the first block. Token-weighted average over the 5 most recent responses across all active sessions (`sum(tokens) / sum(seconds)`), so long responses dominate and TTFT noise averages out. Per-sample sanity filters: >0.3s, 10–800 tps. Recomputed at most every 3s, and only when a transcript changed. Transcripts are read backwards from the end, so large sessions cost only their last few hundred lines.

**API RTT (8)** — Round trip to `api.anthropic.com` over the same network path Claude Code uses (a TUN proxy or `HTTPS_PROXY` is honored, since the probe is `curl`): the time from TLS established to the first response byte, i.e. one request/response over a warm connection. Handshake time is excluded, and so is a local TUN proxy answering ICMP/TCP on the API's behalf — the reason plain `ping` read ~0 ms on proxied machines. One unauthenticated request every 30s in a detached background copy of the binary; the status line shows the median of the last 3 samples and never waits on the network.

**7-day pace (9)** — A weekly percentage alone can't tell you whether you're burning too fast. The second number after the slash is the *pace baseline*: how much of the 7-day window the clock has already consumed, derived from `resets_at` (`(604800 - secondsUntilReset) / 604800`). `15%/62%` means you're well under budget; `34%/33%` means you're ahead of schedule and the used percentage turns red. The bar stays keyed to the absolute percentage, so the bar answers "how much is left" while the number answers "am I too fast". Hidden when the remaining time doesn't fit a 7-day window (plan change, first window).

**Fable weekly (10)** — Queries Anthropic's OAuth usage API for the Fable-scoped weekly limit, reusing the Claude Code OAuth token from the macOS keychain (or `~/.claude/.credentials.json`). The token is passed to curl on stdin, never on the command line. Refreshed in the background at most every 60s. Hidden if the account has no Fable weekly quota.

Caches live in `~/.claude/.sl-*` (session list, TPS, RTT samples, Fable percentage).

## Color Coding

| Metric | Green | Yellow | Red |
|--------|-------|--------|-----|
| Context | <70% used | 70–84% | 85%+ |
| Quota bars | <75% used | 75–89% | 90%+ |
| 7d used % | at or under pace | — | ahead of pace, or 90%+ |
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

Verified on macOS (Apple Silicon). Linux should work as-is; Windows builds but is untested since the move from bash.

## Development

```bash
cargo test
cargo build --release    # target/release/claude-statusline
echo '{"model":{"display_name":"Opus"},"context_window":{"remaining_percentage":60}}' \
  | target/release/claude-statusline
```

## License

[MIT](LICENSE)

## Acknowledgments

- [claudebubble](https://github.com/limin112/claudebubble) — network health detection inspiration
- [Starship](https://starship.rs) / [btop](https://github.com/aristocratos/btop) — color and HUD conventions
