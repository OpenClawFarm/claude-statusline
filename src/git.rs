//! Git branch, dirty markers and a GitHub link, from one `git status` call (two forks total).

use std::path::Path;
use std::process::{Command, Stdio};

pub struct Git {
    pub branch: String,
    /// `~` working tree differs from HEAD, `+` staged changes.
    pub dirty: String,
    pub url: Option<String>,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(["--no-optional-locks", "-c", "gc.auto=0"])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn info(cwd: &str) -> Option<Git> {
    let dir = Path::new(cwd);
    // Outside any work tree, skip git entirely: on macOS even a failing call through the
    // /usr/bin/git shim costs ~10ms.
    if !dir.is_dir() || !dir.ancestors().any(|d| d.join(".git").exists()) {
        return None;
    }
    let status = git(dir, &["status", "--porcelain=v2", "--branch", "-uno"])?;
    let (mut oid, mut head) = ("", "");
    let (mut changed, mut staged) = (false, false);
    for line in status.lines() {
        if let Some(v) = line.strip_prefix("# branch.oid ") {
            oid = v;
        } else if let Some(v) = line.strip_prefix("# branch.head ") {
            head = v;
        } else if let Some(xy) = ["1 ", "2 ", "u "].iter().find_map(|p| line.strip_prefix(p)) {
            changed = true;
            staged |= !xy.starts_with('.');
        }
    }
    let branch = if head == "(detached)" {
        git(dir, &["rev-parse", "--short", "HEAD"])
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    } else {
        head.to_string()
    };
    let mut dirty = String::new();
    // An unborn branch has no HEAD to compare against, which always reads as modified.
    if changed || oid == "(initial)" {
        dirty.push('~');
    }
    if staged {
        dirty.push('+');
    }
    let url = git(dir, &["remote", "get-url", "origin"])
        .map(|s| s.trim_end_matches('\n').to_string())
        .filter(|s| !s.is_empty())
        .map(|u| {
            let u = u.replacen("git@github.com:", "https://github.com/", 1);
            let u = u.strip_suffix(".git").map_or(u.clone(), str::to_string);
            format!("{u}/tree/{branch}")
        });
    Some(Git { branch, dirty, url })
}
