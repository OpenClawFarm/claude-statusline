//! Claude Code statusline — a real-time HUD: model, context, network, throughput, latency,
//! and quotas in one line. Claude Code pipes session JSON on stdin roughly every second.
//!
//! Color scheme inspired by Starship / Lazygit / btop.

mod git;
mod logs;
mod refresh;
mod util;

use serde_json::Value;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use util::{int_part, mtime, now, read_trim, touch, write_atomic};

// Colors (3-tier hierarchy: bold bright → normal → dim)
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const B_CYAN: &str = "\x1b[1;96m";
const B_MAGENTA: &str = "\x1b[1;95m";
const B_BLUE: &str = "\x1b[94m";
const B_WHITE: &str = "\x1b[97m";
const WHITE: &str = "\x1b[37m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const BRIGHT_BLUE: &str = "\x1b[94m";
const BRIGHT_MAG: &str = "\x1b[95m";
const D_SEP: &str = "\x1b[2;90m";
const D_LABEL: &str = "\x1b[2;37m";

const SEVEN_DAYS: i64 = 604_800;

fn main() {
    let home = env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| env::var("USERPROFILE").ok())
        .unwrap_or_default();
    let cache_dir = Path::new(&home).join(".claude");
    match env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("claude-statusline {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some(refresh::RTT) => return refresh::rtt(&cache_dir.join(".sl-rtt")),
        Some(refresh::FABLE) => return refresh::fable(&home, &cache_dir.join(".sl-fable")),
        _ => {}
    }
    let _ = fs::create_dir_all(&cache_dir);
    let mut input = String::new();
    let _ = io::stdin().read_to_string(&mut input);
    let line = render(&input, &home, &cache_dir);
    let mut out = io::stdout().lock();
    let _ = out.write_all(line.as_bytes());
    let _ = out.flush();
}

/// First present value among JSON pointers, jq `a // b // ""` style (null and false skip).
fn field(v: &Value, pointers: &[&str]) -> String {
    for p in pointers {
        match v.pointer(p) {
            None | Some(Value::Null) | Some(Value::Bool(false)) => continue,
            Some(Value::String(s)) => return s.clone(),
            Some(other) => return other.to_string(),
        }
    }
    String::new()
}

fn render(input: &str, home: &str, cache_dir: &Path) -> String {
    let v: Value = serde_json::from_str(input).unwrap_or(Value::Null);
    let mut cwd = field(&v, &["/workspace/current_dir", "/cwd"]);
    if cwd.is_empty() {
        // Logical path like the shell's `pwd` (keeps symlinks such as /tmp unresolved).
        cwd = env::var("PWD")
            .ok()
            .filter(|p| !p.is_empty())
            .or_else(|| env::current_dir().ok().map(|p| p.display().to_string()))
            .unwrap_or_default();
    }
    let model = short_model(&field(&v, &["/model/display_name"]));
    let ctx_remaining = field(&v, &["/context_window/remaining_percentage"]);
    let five_h = field(&v, &["/rate_limits/five_hour/used_percentage"]);
    let five_h_reset = field(&v, &["/rate_limits/five_hour/resets_at"]);
    let seven_d = field(&v, &["/rate_limits/seven_day/used_percentage"]);
    let seven_d_reset = field(&v, &["/rate_limits/seven_day/resets_at"]);
    let now = now();

    let git_part = match git::info(&cwd) {
        Some(g) => match g.url {
            Some(url) => format!(
                " \x1b]8;;{url}\x07{B_MAGENTA} {}{}{RESET}\x1b]8;;\x07",
                g.branch, g.dirty
            ),
            None => format!(" {B_MAGENTA} {}{}{RESET}", g.branch, g.dirty),
        },
        None => String::new(),
    };

    // Everything past the model only shows once Claude Code reports real usage.
    let ctx_rem = if ctx_remaining.is_empty() { None } else { int_part(&ctx_remaining) };
    let has_usage = ctx_rem.is_some_and(|r| r > 0);

    let mut ctx_part = String::new();
    let mut net_part = String::new();
    let mut rl = String::new();
    if let (true, Some(rem)) = (has_usage, ctx_rem) {
        let pct = (100 - rem).clamp(0, 100);
        let total = if model.contains("Opus") || model.contains("opus") { 1000 } else { 200 };
        let c = if pct >= 85 {
            RED
        } else if pct >= 70 {
            YELLOW
        } else {
            GREEN
        };
        ctx_part = format!(" {c}{}k{RESET}", pct * total / 100);
        net_part = network_part(home, cache_dir, now);
        net_part += &rtt_part(cache_dir, now);

        if let Some(f) = int_part(&five_h).filter(|p| (0..=100).contains(p)) {
            rl = format!(" {D_SEP}│{RESET} ⏱ {WHITE}5h{RESET} {} {}", bar(f, 6), cpct(f, None));
            if !five_h_reset.is_empty() {
                rl += &format!(" {}", fmt_reset(&five_h_reset, now));
            }
        }
        if let Some(s) = int_part(&seven_d).filter(|p| (0..=100).contains(p)) {
            let pace = pace_of(&seven_d_reset, SEVEN_DAYS, now);
            rl += &format!(" ☀ {WHITE}7d{RESET} {} {}", bar(s, 6), cpct(s, pace));
            if let Some(p) = pace {
                rl += &format!("{D_SEP}/{RESET}{D_LABEL}{p}%{RESET}");
            }
            if !seven_d_reset.is_empty() {
                rl += &format!(" {}", fmt_reset(&seven_d_reset, now));
            }
        }
        if let Some(fp) = int_part(&fable_pct(cache_dir, now)).filter(|p| (0..=100).contains(p)) {
            rl += &format!(" ✦ {WHITE}Fable{RESET} {} {}", bar(fp, 6), cpct(fp, None));
        }
    }

    let dir = display_dir(&cwd, home);
    format!(
        "\x1b]8;;file://{cwd}\x07{B_CYAN}📂 {dir}{RESET}\x1b]8;;\x07{git_part} {D_SEP}│{RESET} {B_BLUE}{model}{RESET}{}{ctx_part}{net_part}{rl}",
        effort_part(home)
    )
}

/// `~`-relative directory; falls back to stripping `/Users/<name>` or `/home/<name>`.
fn display_dir(cwd: &str, home: &str) -> String {
    let mut dir = match cwd.strip_prefix(home) {
        Some(rest) => format!("~{rest}"),
        None => cwd.to_string(),
    };
    for root in ["/Users/", "/home/"] {
        if let Some(rest) = dir.strip_prefix(root) {
            dir = match rest.find('/') {
                Some(i) => format!("~/{}", &rest[i + 1..]),
                None => format!("~/{dir}"),
            };
        }
    }
    dir
}

/// "Claude Opus 4.6 (1M context)" → "Opus 4.6".
fn short_model(name: &str) -> String {
    let name = if name.is_empty() { "?" } else { name };
    let name = name.replacen("Claude ", "", 1);
    // Drop from the first " (" through the last ")".
    match (name.find(" ("), name.rfind(')')) {
        (Some(open), Some(close)) if open + 2 <= close => {
            format!("{}{}", &name[..open], &name[close + 1..])
        }
        _ => name,
    }
}

fn effort_part(home: &str) -> String {
    let level = fs::read_to_string(Path::new(home).join(".claude/settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .map(|v| field(&v, &["/effortLevel"]))
        .unwrap_or_default();
    let icon = match level.as_str() {
        "low" | "min" => "◔",
        "medium" | "" => "◑",
        "high" | "max" => "◕",
        _ => return String::new(),
    };
    format!(
        " \x1b]8;;file://{home}/.claude/CycleEffort.app\x07{WHITE}{icon}{level}{RESET}\x1b]8;;\x07"
    )
}

/// Network health from the transcripts, plus TPS (recomputed at most every 3s, only when a
/// transcript changed).
fn network_part(home: &str, cache_dir: &Path, now: i64) -> String {
    let mut net = String::from(" 🟢");
    let projects = Path::new(home).join(".claude/projects");
    let logs = logs::active_logs(&projects, &cache_dir.join(".sl-logs"), now);
    if logs.is_empty() {
        return net;
    }
    let n = logs::network(&logs);
    if n.retrying || n.retries > 0 {
        net = if !n.retrying {
            format!(" 🟢{GREEN}{}{RESET}", n.retries)
        } else if n.retries >= 5 {
            format!(" 🔴{RED}{}{RESET}", n.retries)
        } else {
            format!(" 🟡{YELLOW}{}{RESET}", n.retries)
        };
        if let Some(tag) = n.tag {
            net += &format!("{D_LABEL}{tag}{RESET}");
        }
    }

    let tps_cache = cache_dir.join(".sl-tps");
    let tps_mtime = mtime(&tps_cache);
    if mtime(&logs[0]) > tps_mtime
        && now - tps_mtime > 3
        && let Some(v) = logs::tps(&logs).filter(|v| *v > 0)
    {
        write_atomic(&tps_cache, &format!("{v}\n"));
    }
    if let Some(v) = read_trim(&tps_cache).parse::<i64>().ok().filter(|v| *v > 0) {
        net += &format!(" {B_WHITE}{v} tps{RESET}");
    }
    net
}

/// `sort -n` key: leading decimal number, 0 when there is none.
fn sort_n_key(s: &str) -> f64 {
    let s = s.trim_start();
    let end = s
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && c == '-')))
        .map_or(s.len(), |(i, _)| i);
    s[..end].parse().unwrap_or(0.0)
}

/// Median of the last 3 RTT samples; a background probe refreshes them every 5s.
fn rtt_part(cache_dir: &Path, now: i64) -> String {
    let cache = cache_dir.join(".sl-rtt");
    if now - mtime(&cache) > 5 {
        touch(&cache);
        refresh::spawn(refresh::RTT);
    }
    let text = fs::read_to_string(&cache).unwrap_or_default();
    let mut samples: Vec<&str> = text.lines().collect();
    if samples.is_empty() {
        return String::new();
    }
    samples.sort_by(|a, b| sort_n_key(a).total_cmp(&sort_n_key(b)).then_with(|| a.cmp(b)));
    let Some(ms) = samples[samples.len() / 2]
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|v| *v > 0)
    else {
        return String::new();
    };
    let c = if ms >= 500 {
        RED
    } else if ms >= 300 {
        YELLOW
    } else {
        GREEN
    };
    format!(" {c}{ms}ms{RESET}")
}

/// Fable weekly percentage from the cache; a background query refreshes it every 60s.
fn fable_pct(cache_dir: &Path, now: i64) -> String {
    let cache = cache_dir.join(".sl-fable");
    if now - mtime(&cache) > 60 {
        touch(&cache);
        refresh::spawn(refresh::FABLE);
    }
    read_trim(&cache)
}

fn quota_color(pct: i64) -> &'static str {
    if pct >= 90 {
        RED
    } else if pct >= 75 {
        BRIGHT_MAG
    } else {
        BRIGHT_BLUE
    }
}

/// HUD-style colored bar.
fn bar(pct: i64, width: i64) -> String {
    let filled = ((pct * width + 50) / 100).min(width);
    format!(
        "{}{}{DIM}{}{RESET}",
        quota_color(pct),
        "█".repeat(filled as usize),
        "░".repeat((width - filled) as usize)
    )
}

/// Bold percentage; red when ahead of the `pace` baseline.
fn cpct(pct: i64, pace: Option<i64>) -> String {
    let c = if pace.is_some_and(|p| pct > p) { RED } else { quota_color(pct) };
    format!("{BOLD}{c}{pct}%{RESET}")
}

/// How far (in %) the clock has moved through a quota window of `window` seconds, from its
/// reset time. None when unknown or when the remaining time doesn't fit the window (plan
/// change, first window) — better nothing than a baseline on the wrong scale.
fn pace_of(reset: &str, window: i64, now: i64) -> Option<i64> {
    let reset: i64 = reset.trim().parse().ok().filter(|r| *r > 0)?;
    let remain = (reset - now).max(0);
    (remain <= window).then(|| (window - remain) * 100 / window)
}

/// Countdown to a reset epoch: `2d3h`, `2d`, `3h42m`, `12m` or `now`.
fn fmt_reset(epoch: &str, now: i64) -> String {
    let Ok(epoch) = epoch.trim().parse::<i64>() else {
        return String::new();
    };
    let diff = epoch - now;
    let (d, h, m) = (diff / 86_400, diff % 86_400 / 3600, diff % 3600 / 60);
    let text = if diff <= 0 {
        "now".to_string()
    } else if d > 0 && h > 0 {
        format!("{d}d{h}h")
    } else if d > 0 {
        format!("{d}d")
    } else if h > 0 {
        format!("{h}h{m}m")
    } else {
        format!("{m}m")
    };
    format!("{WHITE}{text}{RESET}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names() {
        assert_eq!(short_model("Claude Opus 4.6 (1M context)"), "Opus 4.6");
        assert_eq!(short_model("Opus 5"), "Opus 5");
        assert_eq!(short_model(""), "?");
        assert_eq!(short_model("Sonnet (a) x (b)"), "Sonnet");
        assert_eq!(short_model("odd) name ("), "odd) name (");
    }

    #[test]
    fn dirs() {
        assert_eq!(display_dir("/Users/lee/work", "/Users/lee"), "~/work");
        assert_eq!(display_dir("/Users/lee", "/Users/lee"), "~");
        assert_eq!(display_dir("/Users/other/x", "/Users/lee"), "~/x");
        assert_eq!(display_dir("/home/u/x/y", "/Users/lee"), "~/x/y");
        assert_eq!(display_dir("/opt/x", "/Users/lee"), "/opt/x");
    }

    #[test]
    fn countdowns() {
        let now = 1_000_000;
        let at = |s: i64| (now + s).to_string();
        let plain = |s: String| s.replace(WHITE, "").replace(RESET, "");
        assert_eq!(plain(fmt_reset(&at(-5), now)), "now");
        assert_eq!(plain(fmt_reset(&at(2 * 86_400 + 3 * 3600 + 60), now)), "2d3h");
        assert_eq!(plain(fmt_reset(&at(2 * 86_400 + 60), now)), "2d");
        assert_eq!(plain(fmt_reset(&at(3 * 3600 + 42 * 60), now)), "3h42m");
        assert_eq!(plain(fmt_reset(&at(12 * 60 + 5), now)), "12m");
        assert_eq!(fmt_reset("x", now), "");
    }

    #[test]
    fn pace_baseline() {
        let now = 1_000_000;
        let reset = |pace: i64| (now + SEVEN_DAYS * (100 - pace) / 100).to_string();
        assert_eq!(pace_of(&reset(33), SEVEN_DAYS, now), Some(33));
        assert_eq!(pace_of(&(now + SEVEN_DAYS + 1).to_string(), SEVEN_DAYS, now), None);
        assert_eq!(pace_of("", SEVEN_DAYS, now), None);
        assert!(cpct(34, Some(33)).contains(RED));
        assert!(!cpct(33, Some(33)).contains(RED));
    }

    #[test]
    fn bars() {
        assert_eq!(bar(32, 6), format!("{BRIGHT_BLUE}██{DIM}░░░░{RESET}"));
        assert_eq!(bar(100, 6), format!("{RED}██████{DIM}{RESET}"));
    }
}
