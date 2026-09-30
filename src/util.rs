//! Small filesystem, time and parsing helpers.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Modification time in whole seconds; 0 when the file is missing (so its age is huge).
pub fn mtime(p: &Path) -> i64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Create the file if needed and bump its mtime to now.
pub fn touch(p: &Path) {
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(p) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// Write via a temp file + rename so readers never see a half-written cache.
pub fn write_atomic(p: &Path, content: &str) {
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    if fs::write(&tmp, content).is_ok() && fs::rename(&tmp, p).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

/// File contents without trailing newlines (like shell `$(cat file)`); empty if unreadable.
pub fn read_trim(p: &Path) -> String {
    fs::read_to_string(p)
        .map(|s| s.trim_end_matches('\n').to_string())
        .unwrap_or_default()
}

/// Shell `${v%.*}` followed by an integer test: drop everything from the last '.', parse the rest.
pub fn int_part(s: &str) -> Option<i64> {
    let head = s.rfind('.').map_or(s, |i| &s[..i]);
    head.trim().parse().ok()
}

/// The last `n` lines of a file (like `tail -n`), reading backwards from the end so large
/// transcripts cost only the bytes actually needed.
pub fn tail_lines(path: &Path, n: usize) -> io::Result<Vec<String>> {
    const CHUNK: u64 = 64 * 1024;
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    if len == 0 || n == 0 {
        return Ok(Vec::new());
    }
    let mut pos = len;
    let mut buf: Vec<u8> = Vec::new();
    let mut newlines = 0;
    // n complete lines need n newlines before them, plus the file's own trailing one if present.
    let mut need = n;
    while pos > 0 {
        let size = CHUNK.min(pos);
        pos -= size;
        f.seek(SeekFrom::Start(pos))?;
        let mut chunk = vec![0u8; size as usize];
        f.read_exact(&mut chunk)?;
        if buf.is_empty() && chunk.last() == Some(&b'\n') {
            need += 1;
        }
        newlines += chunk.iter().filter(|&&c| c == b'\n').count();
        chunk.extend_from_slice(&buf);
        buf = chunk;
        if newlines >= need {
            break;
        }
    }
    let body = buf.strip_suffix(b"\n").unwrap_or(&buf);
    let lines: Vec<&[u8]> = body.split(|&c| c == b'\n').collect();
    let start = lines.len().saturating_sub(n);
    Ok(lines[start..]
        .iter()
        .map(|l| String::from_utf8_lossy(l).into_owned())
        .collect())
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// ISO-8601 timestamp (`2026-07-21T03:12:45.123Z`, `+08:00` offsets, no zone = UTC) to epoch
/// microseconds. Fractional digits beyond 6 are truncated, as Python's `fromisoformat` does.
pub fn iso_micros(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let part = s.get(from..to)?;
        part.bytes().all(|c| c.is_ascii_digit()).then(|| part.parse().ok())?
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let mut i = 19;
    let mut micros = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
        for k in 0..6 {
            let digit = if start + k < i { (b[start + k] - b'0') as i64 } else { 0 };
            micros = micros * 10 + digit;
        }
    }
    let offset = match &s[i..] {
        "" | "Z" | "z" => 0,
        tz => {
            let sign = match tz.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hm = tz[1..].replace(':', "");
            if hm.len() != 4 || !hm.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            sign * (hm[..2].parse::<i64>().ok()? * 3600 + hm[2..].parse::<i64>().ok()? * 60)
        }
    };
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset;
    Some(secs * 1_000_000 + micros)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn int_part_matches_shell_suffix_strip() {
        assert_eq!(int_part("45"), Some(45));
        assert_eq!(int_part("45.9"), Some(45));
        assert_eq!(int_part("0.5"), Some(0));
        assert_eq!(int_part(""), None);
        assert_eq!(int_part("abc"), None);
    }

    #[test]
    fn iso_parsing() {
        assert_eq!(iso_micros("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_micros("1970-01-01T00:00:01.5Z"), Some(1_500_000));
        assert_eq!(iso_micros("2026-07-21T03:12:45.123Z"), Some(1_784_603_565_123_000));
        assert_eq!(
            iso_micros("2026-07-21T11:12:45.123+08:00"),
            iso_micros("2026-07-21T03:12:45.123Z")
        );
        assert_eq!(iso_micros("2024-02-29T00:00:00.1234567Z"), Some(1_709_164_800_123_456));
        assert_eq!(iso_micros("not a time"), None);
    }

    fn tmpfile(name: &str, content: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("sl-test-{}-{name}", std::process::id()));
        File::create(&p).unwrap().write_all(content).unwrap();
        p
    }

    #[test]
    fn tail_matches_coreutils() {
        let p = tmpfile("a", b"1\n2\n3\n4\n");
        assert_eq!(tail_lines(&p, 2).unwrap(), ["3", "4"]);
        assert_eq!(tail_lines(&p, 10).unwrap(), ["1", "2", "3", "4"]);
        let p = tmpfile("b", b"1\n2\n3");
        assert_eq!(tail_lines(&p, 2).unwrap(), ["2", "3"]);
        let p = tmpfile("c", b"");
        assert!(tail_lines(&p, 5).unwrap().is_empty());
        // Crosses the 64 KiB chunk boundary.
        let big: String = (0..40_000).map(|i| format!("line-{i}\n")).collect();
        let p = tmpfile("d", big.as_bytes());
        let t = tail_lines(&p, 300).unwrap();
        assert_eq!(t.len(), 300);
        assert_eq!(t[0], "line-39700");
        assert_eq!(t[299], "line-39999");
    }
}
