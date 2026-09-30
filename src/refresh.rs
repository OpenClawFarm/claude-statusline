//! Slow probes (RTT, Fable quota) run in a detached copy of this binary so the status line
//! itself never waits on the network. The foreground only reads their cache files.

use crate::util::write_atomic;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub const RTT: &str = "__refresh-rtt";
pub const FABLE: &str = "__refresh-fable";

const HOST: &str = "api.anthropic.com";

#[cfg(windows)]
const DEV_NULL: &str = "NUL";
#[cfg(not(windows))]
const DEV_NULL: &str = "/dev/null";

/// Re-run this binary with `arg` in the background, detached from the status line's stdio.
pub fn spawn(arg: &str) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe)
            .arg(arg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

fn stdout_of(cmd: &mut Command) -> String {
    cmd.stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// awk's numeric conversion (`printf "%d"`): longest leading decimal prefix, truncated.
fn awk_int(s: &str, scale: f64) -> String {
    let s = s.trim_start();
    let mut end = 0;
    let mut dot = false;
    for (i, c) in s.char_indices() {
        match c {
            '0'..='9' => end = i + 1,
            '.' if !dot => {
                dot = true;
                end = i + 1;
            }
            _ => break,
        }
    }
    let v: f64 = s[..end].parse().unwrap_or(0.0);
    ((v * scale) as i64).to_string()
}

/// One ICMP round trip in whole ms. Empty when no reply line; `"0"` for sub-millisecond
/// replies (which are then discarded, not retried over HTTPS).
#[cfg(not(windows))]
fn ping() -> String {
    let out = stdout_of(Command::new("ping").args(["-c", "1", "-W", "2", HOST]));
    out.match_indices("time=")
        .map(|(i, m)| {
            let rest = &out[i + m.len()..];
            let n = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
            awk_int(&rest[..n], 1.0)
        })
        .collect()
}

#[cfg(windows)]
fn ping() -> String {
    let out = stdout_of(Command::new("ping").args(["-n", "1", "-w", "2000", HOST]));
    out.match_indices("time")
        .filter_map(|(i, m)| {
            let rest = out[i + m.len()..].strip_prefix(['<', '='])?;
            let n = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            (n > 0).then(|| rest[..n].to_string())
        })
        .next()
        .unwrap_or_default()
}

/// HTTPS time-to-first-byte in ms, the fallback when ICMP gets no reply.
fn curl_ttfb() -> String {
    let url = format!("https://{HOST}/v1/messages");
    let out = stdout_of(Command::new("curl").args([
        "-o", DEV_NULL, "-s", "-w", "%{time_starttransfer}", "--max-time", "2", &url,
    ]));
    out.lines()
        .map(|l| awk_int(l.split_whitespace().next().unwrap_or(""), 1000.0))
        .collect()
}

/// Append one RTT sample to `cache`, keeping the last 3 (the display takes their median).
pub fn rtt(cache: &Path) {
    let mut fresh = ping();
    if fresh.is_empty() {
        fresh = curl_ttfb();
    }
    if !fresh.parse::<i64>().is_ok_and(|v| v > 0) {
        return;
    }
    let old = fs::read_to_string(cache).unwrap_or_default();
    let mut samples: Vec<&str> = old.lines().collect();
    samples.push(&fresh);
    let keep = &samples[samples.len().saturating_sub(3)..];
    write_atomic(cache, &format!("{}\n", keep.join("\n")));
}

fn access_token(json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json).ok()?;
    match v.pointer("/claudeAiOauth/accessToken") {
        Some(Value::String(t)) if !t.is_empty() => Some(t.clone()),
        _ => None,
    }
}

/// The Claude Code OAuth token: macOS keychain first, then `~/.claude/.credentials.json`.
fn token(home: &str) -> Option<String> {
    let keychain = stdout_of(Command::new("security").args([
        "find-generic-password",
        "-s",
        "Claude Code-credentials",
        "-w",
    ]));
    access_token(&keychain).or_else(|| {
        let creds = fs::read_to_string(Path::new(home).join(".claude/.credentials.json")).ok()?;
        access_token(&creds)
    })
}

/// `percent` of the first weekly-scoped limit whose model name mentions Fable.
fn fable_percent(usage: &Value) -> Option<String> {
    let limits: Vec<&Value> = match usage.get("limits") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(Value::Object(o)) => o.values().collect(),
        _ => Vec::new(),
    };
    let limit = limits.into_iter().find(|l| {
        l.get("kind").and_then(Value::as_str) == Some("weekly_scoped")
            && l.pointer("/scope/model/display_name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.to_lowercase().contains("fable"))
    })?;
    match limit.get("percent")? {
        Value::Null | Value::Bool(false) => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// Query the OAuth usage API and store the Fable weekly percentage in `cache`.
pub fn fable(home: &str, cache: &Path) {
    let Some(tok) = token(home) else { return };
    // The token goes in on stdin (`-H @-`) so it never shows up in the process list.
    let child = Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "3",
            "-H",
            "@-",
            "-H",
            "anthropic-beta: oauth-2025-04-20",
            &format!("https://{HOST}/api/oauth/usage"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "Authorization: Bearer {tok}");
    }
    let Ok(out) = child.wait_with_output() else { return };
    let pct = serde_json::from_slice::<Value>(&out.stdout)
        .ok()
        .and_then(|v| fable_percent(&v));
    if let Some(pct) = pct {
        write_atomic(cache, &format!("{pct}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn awk_conversion() {
        assert_eq!(awk_int("221.345", 1.0), "221");
        assert_eq!(awk_int("0.211", 1.0), "0");
        assert_eq!(awk_int("", 1.0), "0");
        assert_eq!(awk_int("0.454168", 1000.0), "454");
    }

    #[test]
    fn fable_limit_selection() {
        let usage = json!({"limits": [
            {"kind": "weekly", "percent": 10},
            {"kind": "weekly_scoped", "scope": {"model": {"display_name": "Opus"}}, "percent": 20},
            {"kind": "weekly_scoped", "scope": {"model": {"display_name": "Claude FABLE 5"}}, "percent": 28.5},
        ]});
        assert_eq!(fable_percent(&usage).as_deref(), Some("28.5"));
        assert_eq!(fable_percent(&json!({"limits": []})), None);
        assert_eq!(fable_percent(&json!({})), None);
    }
}
