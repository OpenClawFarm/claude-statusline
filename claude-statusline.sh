#!/bin/bash
# Claude Code Statusline - designed for leecz
# Version: 2.0.0
# Color scheme inspired by Starship / Lazygit / btop
# Optimized: ~45 forks → ~12 forks per refresh

input=$(cat)

# -- Colors (3-tier hierarchy: bold bright → normal → dim) --
bold='\033[1m'
dim='\033[2m'
reset='\033[0m'
b_cyan='\033[1;96m'
b_magenta='\033[1;95m'
b_blue='\033[94m'
b_white='\033[97m'
green='\033[32m'
yellow='\033[33m'
red='\033[31m'
bright_blue='\033[94m'
bright_mag='\033[95m'
d_sep='\033[2;90m'
d_label='\033[2;37m'

# -- Parse all CC JSON fields in a single jq call --
IFS=$'\t' read -r cwd model ctx_remaining five_h five_h_reset seven_d seven_d_reset cost_usd <<< \
    "$(echo "$input" | jq -r '[
        (.workspace.current_dir // .cwd // ""),
        (.model.display_name // ""),
        (.context_window.remaining_percentage // ""),
        (.rate_limits.five_hour.used_percentage // ""),
        (.rate_limits.five_hour.resets_at // ""),
        (.rate_limits.seven_day.used_percentage // ""),
        (.rate_limits.seven_day.resets_at // ""),
        (.cost.total_cost_usd // "")
    ] | @tsv')"

[ -z "$cwd" ] && cwd=$(pwd)
dir="${cwd/#$HOME/~}"
model="${model:-?}"
model="${model/Claude /}"
model="${model/ (*)/}"

# HUD-style colored bar (no seq — pure bash)
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
    local bar_filled="" bar_empty=""
    printf -v bar_filled '%*s' "$filled" ''; bar_filled="${bar_filled// /█}"
    printf -v bar_empty '%*s' "$empty" ''; bar_empty="${bar_empty// /░}"
    printf "%b" "${c}${bar_filled}${dim}${bar_empty}${reset}"
}

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

# -- Git branch --
git_part=""
if cd "$cwd" 2>/dev/null && git -c gc.auto=0 rev-parse --is-inside-work-tree &>/dev/null; then
    branch=$(git -c gc.auto=0 symbolic-ref --short HEAD 2>/dev/null \
             || git -c gc.auto=0 rev-parse --short HEAD 2>/dev/null)
    dirty=""
    git -c gc.auto=0 diff-index --quiet HEAD -- 2>/dev/null || dirty="~"
    git -c gc.auto=0 diff --cached --quiet 2>/dev/null || dirty="${dirty}+"
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

# -- Effort level --
effort_level=$(jq -r '.effortLevel // empty' ~/.claude/settings.json 2>/dev/null)
case "$effort_level" in
    low|min)     effort_icon="◔" ;;
    medium|"")   effort_icon="◑" ;;
    high|max)    effort_icon="◕" ;;
    *)           effort_icon="" ;;
esac
effort_part=""
[ -n "$effort_icon" ] && effort_part=" \e]8;;file://${HOME}/.claude/CycleEffort.app\a\033[37m${effort_icon}${effort_level}${reset}\e]8;;\a"

# -- Context Window (Xk) --
ctx_part=""
if [ -n "$ctx_remaining" ]; then
    ctx_pct=$(( 100 - ${ctx_remaining%.*} ))
    [ "$ctx_pct" -lt 0 ] && ctx_pct=0
    [ "$ctx_pct" -gt 100 ] && ctx_pct=100
    case "$model" in
        *Opus*|*opus*) ctx_total=1000 ;;
        *)             ctx_total=200 ;;
    esac
    ctx_k=$(( ctx_pct * ctx_total / 100 ))
    if [ "$ctx_pct" -ge 85 ]; then ctx_c="${red}"
    elif [ "$ctx_pct" -ge 70 ]; then ctx_c="${yellow}"
    else ctx_c="${green}"; fi
    ctx_part=" ${ctx_c}${ctx_k}k${reset}"
fi

# -- Network health + TPS + RTT --
net_part=" 🟢"

# Find ALL active sessions + subagents (cached 10s)
# TPS/RTT reflect API backend quality — more sessions = better signal
log_cache="/tmp/.claude-statusline-logs"
log_cache_age=$(( $(date +%s) - $(stat -f %m "$log_cache" 2>/dev/null || echo 0) ))
if [ "$log_cache_age" -gt 10 ] || [ ! -f "$log_cache" ]; then
    find "$HOME/.claude/projects" -maxdepth 4 -name "*.jsonl" \
        -mmin -5 2>/dev/null \
        | xargs command ls -1t 2>/dev/null | head -10 > "$log_cache"
fi
active_logs=()
while IFS= read -r f; do
    [ -f "$f" ] && active_logs+=("$f")
done < "$log_cache"

if [ "${#active_logs[@]}" -gt 0 ]; then
    # Concat tail from all active sessions for broader signal
    tail_buf=$(tail -100 "${active_logs[@]}" 2>/dev/null)

    # Network retry detection — pure bash, no grep forks
    last_err_ln=0
    last_ok_ln=0
    retry_count=0
    has_cert=0 has_rst=0 has_504=0
    ln=0
    while IFS= read -r line; do
        ((ln++))
        if [[ "$line" == *'"retryInMs"'* ]]; then
            last_err_ln=$ln
            ((retry_count++))
            [[ "$line" == *CERTIFICATE* || "$line" == *ERR_TLS* ]] && has_cert=1
            [[ "$line" == *ECONNRESET* ]] && has_rst=1
            [[ "$line" == *'"504"'* ]] && has_504=1
        fi
        [[ "$line" == *'"stop_reason"'* ]] && last_ok_ln=$ln
    done <<< "$tail_buf"

    if [ "$last_err_ln" -gt "$last_ok_ln" ] && [ "$last_err_ln" -gt 0 ]; then
        err_tag=""
        [ "$has_cert" -eq 1 ] && err_tag="cert"
        [ "$has_rst" -eq 1 ] && [ -z "$err_tag" ] && err_tag="rst"
        [ "$has_504" -eq 1 ] && [ -z "$err_tag" ] && err_tag="504"
        if [ "$retry_count" -ge 5 ]; then
            net_part=" 🔴${red}${retry_count}${reset}"
        else
            net_part=" 🟡${yellow}${retry_count}${reset}"
        fi
        [ -n "$err_tag" ] && net_part="${net_part}${d_label}${err_tag}${reset}"
    fi

    # -- TPS: recompute when any log has changed --
    tps_cache="/tmp/.claude-statusline-tps"
    newest_mtime=$(stat -f %m "${active_logs[0]}" 2>/dev/null || echo 0)
    tps_mtime=$(stat -f %m "$tps_cache" 2>/dev/null || echo 0)
    if [ "$newest_mtime" -gt "$tps_mtime" ]; then
        tps_val=$(tail -300 "${active_logs[@]}" 2>/dev/null | python3 -c "
import sys, json, os
from datetime import datetime
prev_ts = None
samples = []
cache_path = '/tmp/.claude-statusline-tps-history'
cached_count = 0
if os.path.exists(cache_path):
    with open(cache_path) as f:
        cached_count = len(f.read().strip().splitlines())
min_tokens = 10 if cached_count < 5 else 100
for line in sys.stdin:
    try: d = json.loads(line)
    except: continue
    ts = d.get('timestamp')
    if d.get('type') == 'assistant' and d.get('message',{}).get('stop_reason') and prev_ts:
        ot = d.get('message',{}).get('usage',{}).get('output_tokens',0)
        if ot >= min_tokens:
            t1 = datetime.fromisoformat(prev_ts.replace('Z','+00:00'))
            t2 = datetime.fromisoformat(ts.replace('Z','+00:00'))
            dt = (t2-t1).total_seconds()
            if dt > 0.3:
                tps = int(ot/dt)
                if 10 <= tps <= 500: samples.append(tps)
    if ts: prev_ts = ts
if samples:
    recent = samples[-3:]
    median = sorted(recent)[len(recent)//2]
    with open(cache_path, 'a') as f: f.write(str(median) + '\n')
    with open(cache_path) as f: lines = f.read().strip().splitlines()
    if len(lines) > 10:
        with open(cache_path, 'w') as f: f.write('\n'.join(lines[-10:]) + '\n')
    print(median)
" 2>/dev/null)
        if [ -n "$tps_val" ] && [ "$tps_val" -gt 0 ] 2>/dev/null; then
            echo "$tps_val" > "$tps_cache"
        fi
    fi
    tps_val=$(cat "$tps_cache" 2>/dev/null)
    [ -n "$tps_val" ] && [ "$tps_val" -gt 0 ] 2>/dev/null && \
        net_part="${net_part} ${b_white}${tps_val} tps${reset}"
fi

# -- API RTT: ping with adaptive interval, sliding window median --
rtt_cache="/tmp/.claude-statusline-rtt"
rtt_age=$(( $(date +%s) - $(stat -f %m "$rtt_cache" 2>/dev/null || echo 0) ))
if [ "$rtt_age" -gt 5 ]; then
    # Single packet, 2s timeout — never blocks more than 2s
    rtt_fresh=$(ping -c 1 -W 2 api.anthropic.com 2>/dev/null \
        | grep -o 'time=[0-9.]*' | cut -d= -f2 | awk '{printf "%d", $1}')
    [ -z "$rtt_fresh" ] && rtt_fresh=$(curl -o /dev/null -s -w '%{time_starttransfer}' \
        --max-time 2 https://api.anthropic.com/v1/messages 2>/dev/null \
        | awk '{printf "%d", $1*1000}')
    if [ -n "$rtt_fresh" ] && [ "$rtt_fresh" -gt 0 ] 2>/dev/null; then
        { cat "$rtt_cache" 2>/dev/null; echo "$rtt_fresh"; } | tail -3 > "$rtt_cache.tmp" \
            && mv "$rtt_cache.tmp" "$rtt_cache"
    fi
fi
rtt_ms=""
if [ -f "$rtt_cache" ]; then
    rtt_ms=$(sort -n "$rtt_cache" | awk '{a[NR]=$1} END{print a[int(NR/2)+1]}')
fi
if [ -n "$rtt_ms" ] && [ "$rtt_ms" -gt 0 ] 2>/dev/null; then
    if [ "$rtt_ms" -ge 500 ]; then rtt_c="${red}"
    elif [ "$rtt_ms" -ge 300 ]; then rtt_c="${yellow}"
    else rtt_c="${green}"; fi
    net_part="${net_part} ${rtt_c}${rtt_ms}ms${reset}"
fi

# -- Rate limits --
rl=""
fmt_reset() {
    local epoch=$1
    [ -z "$epoch" ] && return
    local now=$(date +%s)
    local diff=$(( epoch - now ))
    [ "$diff" -le 0 ] && { printf "%b" "\033[37mnow${reset}"; return; }
    local d=$(( diff / 86400 )) h=$(( (diff % 86400) / 3600 )) m=$(( (diff % 3600) / 60 ))
    if [ "$d" -gt 0 ] && [ "$h" -gt 0 ]; then
        printf "%b" "\033[37m${d}d${h}h${reset}"
    elif [ "$d" -gt 0 ]; then
        printf "%b" "\033[37m${d}d${reset}"
    elif [ "$h" -gt 0 ]; then
        printf "%b" "\033[37m${h}h${m}m${reset}"
    else
        printf "%b" "\033[37m${m}m${reset}"
    fi
}
if [ -n "$five_h" ]; then
    f=${five_h%.*}
    if [ "$f" -ge 0 ] 2>/dev/null && [ "$f" -le 100 ]; then
        rl=" ${d_sep}│${reset} \033[37m5h${reset} $(bar $f 6 quota) $(cpct $f quota)"
        [ -n "$five_h_reset" ] && rl="${rl}$(printf ' '; fmt_reset "$five_h_reset")"
    fi
fi
if [ -n "$seven_d" ]; then
    s=${seven_d%.*}
    if [ "$s" -ge 0 ] 2>/dev/null && [ "$s" -le 100 ]; then
        rl="${rl} \033[37m7d${reset} $(bar $s 6 quota) $(cpct $s quota)"
        [ -n "$seven_d_reset" ] && rl="${rl}$(printf ' '; fmt_reset "$seven_d_reset")"
    fi
fi

# -- Cost --
cost_part=""
if [ -n "$cost_usd" ]; then
    cost_int=${cost_usd%%.*}
    if [ "${cost_int:-0}" -lt 1 ]; then
        cost_fmt=$(printf '%.1f' "$cost_usd")
    else
        cost_fmt=$(printf '%.0f' "$cost_usd")
    fi
    cost_part=" ${yellow}\$${cost_fmt}${reset}"
fi

# -- Assemble --
dir_link="\e]8;;file://${cwd}\a${b_cyan}📂 ${dir}${reset}\e]8;;\a"
printf "%b" "${dir_link}${git_part} ${d_sep}│${reset} ${b_blue}${model}${reset}${effort_part}${ctx_part}${net_part}${rl}${cost_part}"
