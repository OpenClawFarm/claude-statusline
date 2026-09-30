//! Session transcripts (JSONL): discovery, network-retry detection and TPS.

use crate::util::{iso_micros, mtime, tail_lines, write_atomic};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_LOGS: usize = 10;
const MAX_DEPTH: u32 = 4;
const ACTIVE_SECS: i64 = 300;

/// Active transcripts (sessions and subagents) under `projects`: `*.jsonl` written in the last
/// few minutes, newest first, at most 10. The list is cached in `cache` for 10s.
pub fn active_logs(projects: &Path, cache: &Path, now: i64) -> Vec<PathBuf> {
    if now - mtime(cache) > 10 || !cache.exists() {
        let mut found = Vec::new();
        scan(projects, 1, now, &mut found);
        found.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.as_os_str().cmp(a.1.as_os_str()))
        });
        let list: String = found
            .iter()
            .take(MAX_LOGS)
            .map(|(_, p)| format!("{}\n", p.display()))
            .collect();
        write_atomic(cache, &list);
    }
    fs::read_to_string(cache)
        .unwrap_or_default()
        .lines()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .collect()
}

fn scan(dir: &Path, depth: u32, now: i64, out: &mut Vec<(i64, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let path = e.path();
        if ft.is_dir() {
            if depth < MAX_DEPTH {
                scan(&path, depth + 1, now, out);
            }
        } else if ft.is_file() && e.file_name().to_string_lossy().ends_with(".jsonl") {
            let m = mtime(&path);
            // Same window as macOS `find -mmin -5`: written within the last 300 whole seconds.
            if now - m <= ACTIVE_SECS {
                out.push((m, path));
            }
        }
    }
}

pub struct Net {
    pub retries: u32,
    /// The last retry came after the last completed response.
    pub retrying: bool,
    pub tag: Option<&'static str>,
}

/// Positional retry detection over the last 100 lines of each transcript: retrying when the
/// last `retryInMs` appears after the last `stop_reason`.
pub fn network(logs: &[PathBuf]) -> Net {
    let (mut ln, mut last_err, mut last_ok, mut retries) = (0usize, 0usize, 0usize, 0u32);
    let (mut cert, mut rst, mut gw504) = (false, false, false);
    for p in logs {
        for line in tail_lines(p, 100).unwrap_or_default() {
            ln += 1;
            if line.contains("\"retryInMs\"") {
                last_err = ln;
                retries += 1;
                cert |= line.contains("CERTIFICATE") || line.contains("ERR_TLS");
                rst |= line.contains("ECONNRESET");
                gw504 |= line.contains("\"504\"");
            }
            if line.contains("\"stop_reason\"") {
                last_ok = ln;
            }
        }
    }
    let tag = if cert {
        Some("cert")
    } else if rst {
        Some("rst")
    } else if gw504 {
        Some("504")
    } else {
        None
    };
    Net { retries, retrying: last_err > last_ok && last_err > 0, tag }
}

#[derive(Deserialize)]
struct Record {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Deserialize, Default)]
struct Message {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    output_tokens: Option<f64>,
}

/// Output tokens per second of streaming, token-weighted over the 5 most recent responses.
///
/// One API response spans several assistant records (thinking/text/tool_use) sharing
/// `message.id` and `usage`; each is timed from the record just before its first block, and
/// only the last record per id (the full span) is kept. See the TPS algorithm notes in README.
pub fn tps(logs: &[PathBuf]) -> Option<i64> {
    // message id -> (end timestamp, output tokens, streaming seconds)
    let mut seen: HashMap<Option<String>, (String, f64, f64)> = HashMap::new();
    for path in logs {
        let Ok(lines) = tail_lines(path, 300) else { continue };
        let mut prev_ts: Option<String> = None;
        let mut cur_id: Option<String> = None;
        let mut cur_start: Option<String> = None;
        for line in lines {
            let Ok(rec) = serde_json::from_str::<Record>(&line) else { continue };
            let ts = rec.timestamp.filter(|t| !t.is_empty());
            if rec.kind.as_deref() == Some("assistant")
                && let Some(ts) = &ts
            {
                let msg = rec.message.unwrap_or_default();
                if msg.id != cur_id {
                    cur_id = msg.id;
                    cur_start = prev_ts.clone();
                }
                if msg.stop_reason.is_some_and(|s| !s.is_empty())
                    && let Some(start) = &cur_start
                {
                    let ot = msg.usage.and_then(|u| u.output_tokens).unwrap_or(0.0);
                    if let (Some(end_us), Some(start_us)) = (iso_micros(ts), iso_micros(start)) {
                        let dt = (end_us - start_us) as f64 / 1e6;
                        if ot > 0.0 && dt > 0.3 && (10.0..=800.0).contains(&(ot / dt)) {
                            seen.insert(cur_id.clone(), (ts.clone(), ot, dt));
                        }
                    }
                }
            }
            if ts.is_some() {
                prev_ts = ts;
            }
        }
    }
    let mut samples: Vec<_> = seen.into_values().collect();
    // ISO-8601 UTC strings sort chronologically.
    samples.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.total_cmp(&b.2))
    });
    let recent = &samples[samples.len().saturating_sub(5)..];
    let tok: f64 = recent.iter().map(|s| s.1).sum();
    let sec: f64 = recent.iter().map(|s| s.2).sum();
    (tok >= 100.0).then(|| (tok / sec) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn transcript(name: &str, lines: &[String]) -> PathBuf {
        let p = std::env::temp_dir().join(format!("sl-logs-{}-{name}.jsonl", std::process::id()));
        let mut f = fs::File::create(&p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        p
    }

    fn user(ts: &str) -> String {
        format!(r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user"}}}}"#)
    }

    fn assistant(ts: &str, id: &str, tokens: u32) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","stop_reason":"end_turn","usage":{{"output_tokens":{tokens}}}}}}}"#
        )
    }

    #[test]
    fn tps_groups_blocks_by_message_id() {
        // 1000 tokens over 10s, written as three blocks of the same response.
        let p = transcript(
            "grouped",
            &[
                user("2026-07-21T00:00:00.000Z"),
                assistant("2026-07-21T00:00:04.000Z", "m1", 1000),
                assistant("2026-07-21T00:00:07.000Z", "m1", 1000),
                assistant("2026-07-21T00:00:10.000Z", "m1", 1000),
            ],
        );
        assert_eq!(tps(&[p]), Some(100));
    }

    #[test]
    fn tps_is_token_weighted_over_recent_five() {
        let mut lines = Vec::new();
        for i in 0..7 {
            let base = i * 100;
            lines.push(user(&format!("2026-07-21T00:{:02}:{:02}.000Z", base / 60, base % 60)));
            let end = base + 10;
            lines.push(assistant(
                &format!("2026-07-21T00:{:02}:{:02}.000Z", end / 60, end % 60),
                &format!("m{i}"),
                if i < 2 { 7000 } else { 500 },
            ));
        }
        // The two fast early samples (700 tps) fall outside the last five (50 tps each).
        assert_eq!(tps(&[transcript("weighted", &lines)]), Some(50));
    }

    #[test]
    fn tps_needs_enough_tokens_and_sane_samples() {
        let few = transcript(
            "few",
            &[user("2026-07-21T00:00:00.000Z"), assistant("2026-07-21T00:00:02.000Z", "a", 50)],
        );
        assert_eq!(tps(&[few]), None);
        // 0.2s span is below the 0.3s floor.
        let short = transcript(
            "short",
            &[user("2026-07-21T00:00:00.000Z"), assistant("2026-07-21T00:00:00.200Z", "b", 100)],
        );
        assert_eq!(tps(&[short]), None);
    }

    #[test]
    fn network_positional_retry_state() {
        let retry = r#"{"type":"system","retryInMs":500,"error":"ECONNRESET"}"#.to_string();
        let ok = r#"{"type":"assistant","message":{"stop_reason":"end_turn"}}"#.to_string();
        let retrying = transcript("retrying", &[ok.clone(), retry.clone(), retry.clone()]);
        let n = network(&[retrying]);
        assert!(n.retrying);
        assert_eq!((n.retries, n.tag), (2, Some("rst")));
        let recovered = transcript("recovered", &[retry, ok]);
        let n = network(&[recovered]);
        assert!(!n.retrying);
        assert_eq!(n.retries, 1);
    }
}
