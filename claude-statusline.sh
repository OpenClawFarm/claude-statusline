#!/bin/bash
# Claude Code Statusline - designed for leecz
# Version: 1.0.0
# Color scheme inspired by Starship / Lazygit / btop

input=$(cat)

# -- Colors (3-tier hierarchy: bold bright → normal → dim) --
bold='\033[1m'
dim='\033[2m'
reset='\033[0m'
# Bright variants for primary info
b_cyan='\033[1;96m'      # directory (starship convention)
b_magenta='\033[1;95m'    # git branch (lazygit convention)
b_blue='\033[94m'          # model name (no bold)
b_white='\033[97m'         # key values (percentages, no bold)
# Semantic bar colors
green='\033[32m'
yellow='\033[33m'
red='\033[31m'
bright_blue='\033[94m'
bright_mag='\033[95m'
# Dim elements
d_sep='\033[2;90m'        # separators (dark gray, dim)
d_label='\033[2;37m'      # labels & context text

# HUD-style colored bar: bar <percent> <width> <type>
bar() {
    local pct=${1:-0} w=${2:-8} type=${3:-ctx}
    local filled=$(( (pct * w + 50) / 100 ))
    [ "$filled" -gt "$w" ] && filled=$w
    local empty=$(( w - filled ))
    local c
    if [ "$type" = "quota" ]; then
        if [ "$pct" -ge 90 ]; then c="${red}"
        elif [ "$pct" -ge 75 ]; then c="${bright_mag}"
        else c="${bright_blue}"; fi
    else
        if [ "$pct" -ge 85 ]; then c=$red
        elif [ "$pct" -ge 70 ]; then c=$yellow
        else c=$green; fi
    fi
    local bar_str=""
    [ "$filled" -gt 0 ] && bar_str="${c}$(printf '█%.0s' $(seq 1 $filled))"
    [ "$empty" -gt 0 ] && bar_str="${bar_str}${dim}$(printf '░%.0s' $(seq 1 $empty))"
    printf "%b" "${bar_str}${reset}"
}

# Colorize percent value based on threshold
cpct() {
    local pct=${1:-0} type=${2:-ctx}
    local c
    if [ "$type" = "quota" ]; then
        if [ "$pct" -ge 90 ]; then c="${red}"
        elif [ "$pct" -ge 75 ]; then c="${bright_mag}"
        else c="${bright_blue}"; fi
    else
        if [ "$pct" -ge 85 ]; then c="${red}"
        elif [ "$pct" -ge 70 ]; then c="${yellow}"
        else c=$b_white; fi
    fi
    printf "%b" "${bold}${c}$(printf '%d' $pct)%${reset}"
}

# -- Directory --
cwd=$(echo "$input" | jq -r '.workspace.current_dir // .cwd // empty')
[ -z "$cwd" ] && cwd=$(pwd)
dir="${cwd/#$HOME/~}"

# -- Git branch --
git_part=""
if cd "$cwd" 2>/dev/null && git -c gc.auto=0 rev-parse --is-inside-work-tree &>/dev/null; then
    branch=$(git -c gc.auto=0 symbolic-ref --short HEAD 2>/dev/null \
             || git -c gc.auto=0 rev-parse --short HEAD 2>/dev/null)
    dirty=""
    git -c gc.auto=0 diff-index --quiet HEAD -- 2>/dev/null || dirty="~"
    git -c gc.auto=0 diff --cached --quiet 2>/dev/null || dirty="${dirty}+"
    # Build GitHub branch URL from remote
    remote_url=$(git -c gc.auto=0 remote get-url origin 2>/dev/null)
    gh_url=""
    if [ -n "$remote_url" ]; then
        gh_url=$(echo "$remote_url" | sed 's|git@github.com:|https://github.com/|' | sed 's|\.git$||')
        gh_url="${gh_url}/tree/${branch}"
    fi
    if [ -n "$gh_url" ]; then
        git_part=" \e]8;;${gh_url}\a${b_magenta} ${branch}${dirty}${reset}\e]8;;\a"
    else
        git_part=" ${b_magenta} ${branch}${dirty}${reset}"
    fi
fi

# -- Model --
model=$(echo "$input" | jq -r '.model.display_name // empty')
model="${model:-?}"
model="${model/Claude /}"
model="${model/ (*)/}"

# -- Effort level indicator (read from settings.json) --
effort_level=$(jq -r '.effortLevel // empty' ~/.claude/settings.json 2>/dev/null)
case "$effort_level" in
    low|min)     effort_icon="◔" ;;
    medium|"")   effort_icon="◑" ;;
    high|max)    effort_icon="◕" ;;
    *)           effort_icon="" ;;
esac
effort_part=""
[ -n "$effort_icon" ] && effort_part=" \e]8;;file://${HOME}/.claude/CycleEffort.app\a\033[37m${effort_icon}${effort_level}${reset}\e]8;;\a"

# -- Context Window usage (show as Xk) --
ctx_part=""
ctx_remaining=$(echo "$input" | jq -r '.context_window.remaining_percentage // empty')
if [ -n "$ctx_remaining" ]; then
    ctx_pct=$(( 100 - ${ctx_remaining%.*} ))
    [ "$ctx_pct" -lt 0 ] && ctx_pct=0
    [ "$ctx_pct" -gt 100 ] && ctx_pct=100
    # Determine total context size from model
    case "$model" in
        *Opus*|*opus*) ctx_total=1000 ;;
        *)             ctx_total=200 ;;
    esac
    ctx_k=$(( ctx_pct * ctx_total / 100 ))
    # Color by usage level
    if [ "$ctx_pct" -ge 85 ]; then ctx_c="${red}"
    elif [ "$ctx_pct" -ge 70 ]; then ctx_c="${yellow}"
    else ctx_c="${green}"; fi
    ctx_part=" ${ctx_c}${ctx_k}k${reset}"
fi

# -- Network health (from CC session JSONL logs, inspired by claudebubble) --
# 🟢 = healthy, 🟡 = light retries (1-4), 🔴 = heavy retries (5+)
net_part=" 🟢"
# Find the most recently modified JSONL across all projects (not subagents)
# Handles cwd changes, /add-dir, worktrees — always finds the active session
latest_log=$(find "$HOME/.claude/projects" -maxdepth 2 -name "*.jsonl" \
    ! -path "*/subagents/*" -mmin -5 2>/dev/null \
    | xargs command ls -1t 2>/dev/null | head -1)
if [ -n "$latest_log" ]; then
    tail_buf=$(tail -100 "$latest_log" 2>/dev/null)
    # Compare position: last retry vs last successful response
    last_err_ln=$(echo "$tail_buf" | grep -n '"retryInMs"' | tail -1 | cut -d: -f1)
    last_ok_ln=$(echo "$tail_buf" | grep -n '"stop_reason"' | tail -1 | cut -d: -f1)
    last_err_ln=${last_err_ln:-0}
    last_ok_ln=${last_ok_ln:-0}
    if [ "$last_err_ln" -gt "$last_ok_ln" ] && [ "$last_err_ln" -gt 0 ]; then
        retry_count=$(echo "$tail_buf" | grep -c '"retryInMs"')
        err_tag=""
        if echo "$tail_buf" | grep -q 'CERTIFICATE\|ERR_TLS'; then err_tag="cert"
        elif echo "$tail_buf" | grep -q 'ECONNRESET'; then err_tag="rst"
        elif echo "$tail_buf" | grep -q '"504"'; then err_tag="504"; fi
        if [ "$retry_count" -ge 5 ]; then
            net_part=" 🔴${red}${retry_count}${reset}"
        else
            net_part=" 🟡${yellow}${retry_count}${reset}"
        fi
        [ -n "$err_tag" ] && net_part="${net_part}${d_label}${err_tag}${reset}"
    fi
    # -- TPS: wider tail (300 lines) + cache for tool-heavy turns --
    tps_cache="/tmp/.claude-statusline-tps"
    tps_val=$(tail -300 "$latest_log" 2>/dev/null | python3 -c "
import sys, json
from datetime import datetime
last_user_ts = None
best = None
for line in sys.stdin:
    try: d = json.loads(line)
    except: continue
    if d.get('type') == 'user':
        last_user_ts = d.get('timestamp')
    if d.get('type') == 'assistant' and d.get('message',{}).get('stop_reason') and last_user_ts:
        ot = d.get('message',{}).get('usage',{}).get('output_tokens',0)
        if ot > 10:
            best = (last_user_ts, d['timestamp'], ot)
if best:
    t1 = datetime.fromisoformat(best[0].replace('Z','+00:00'))
    t2 = datetime.fromisoformat(best[1].replace('Z','+00:00'))
    dt = (t2-t1).total_seconds()
    if dt > 0: print(int(best[2]/dt))
" 2>/dev/null)
    if [ -n "$tps_val" ] && [ "$tps_val" -gt 0 ] 2>/dev/null; then
        echo "$tps_val" > "$tps_cache"
    elif [ -f "$tps_cache" ]; then
        tps_val=$(cat "$tps_cache")
    fi
    [ -n "$tps_val" ] && [ "$tps_val" -gt 0 ] 2>/dev/null && \
        net_part="${net_part} ${b_white}${tps_val}tps${reset}"
fi

# -- Rate limits (5h / 7d) with reset countdown --
rl=""
five_h=$(echo "$input" | jq -r '.rate_limits.five_hour.used_percentage // empty')
five_h_reset=$(echo "$input" | jq -r '.rate_limits.five_hour.resets_at // empty')
seven_d=$(echo "$input" | jq -r '.rate_limits.seven_day.used_percentage // empty')
seven_d_reset=$(echo "$input" | jq -r '.rate_limits.seven_day.resets_at // empty')

# Format reset epoch → human-readable countdown
fmt_reset() {
    local epoch=$1
    [ -z "$epoch" ] && return
    local now=$(date +%s)
    local diff=$(( epoch - now ))
    [ "$diff" -le 0 ] && { printf "%b" "\033[37mnow${reset}"; return; }
    local d=$(( diff / 86400 )) h=$(( (diff % 86400) / 3600 )) m=$(( (diff % 3600) / 60 ))
    if [ "$d" -gt 0 ]; then
        printf "%b" "\033[37m${d}d${h}h${reset}"
    elif [ "$h" -gt 0 ]; then
        printf "%b" "\033[37m${h}h${m}m${reset}"
    else
        printf "%b" "\033[37m${m}m${reset}"
    fi
}

if [ -n "$five_h" ]; then
    f=${five_h%.*}
    rl=" ${d_sep}│${reset} \033[37m5h${reset} $(bar $f 6 quota) $(cpct $f quota)"
    [ -n "$five_h_reset" ] && rl="${rl}$(printf ' '; fmt_reset "$five_h_reset")"
fi
if [ -n "$seven_d" ]; then
    s=${seven_d%.*}
    rl="${rl} \033[37m7d${reset} $(bar $s 6 quota) $(cpct $s quota)"
    [ -n "$seven_d_reset" ] && rl="${rl}$(printf ' '; fmt_reset "$seven_d_reset")"
fi

# -- Cost --
cost_part=""
cost_usd=$(echo "$input" | jq -r '.cost.total_cost_usd // empty')
if [ -n "$cost_usd" ]; then
    cost_fmt=$(printf '%.0f' "$cost_usd")
    cost_part=" ${yellow}\$${cost_fmt}${reset}"
fi

# -- Clickable directory (OSC 8 hyperlink → file:// URI) --
dir_link="\e]8;;file://${cwd}\a${b_cyan}📂 ${dir}${reset}\e]8;;\a"

# -- Assemble --
printf "%b" "${dir_link}${git_part} ${d_sep}│${reset} ${b_blue}${model}${reset}${effort_part}${ctx_part}${net_part}${rl}${cost_part}"
