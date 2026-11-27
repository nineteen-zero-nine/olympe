use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};

fn output(dir: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new("git").arg("-C").arg(dir).args(args).output()
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = output(dir, args).ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run git in `dir` and return what it prints. A failure carries what git
/// said.
pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let out = output(dir, args).context("could not run git; is it installed?")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether git, run in `dir`, succeeds: for calls whose failure is an answer.
pub fn succeeds(dir: &Path, args: &[&str]) -> bool {
    output(dir, args).is_ok_and(|out| out.status.success())
}

/// The files under `dir` that differ from the last commit, untracked ones
/// included, relative to `dir`. `None` when `dir` is not in a git repository.
pub fn changed_paths(dir: &Path) -> Option<BTreeSet<String>> {
    // git prints paths from the top of the repository; `dir` may be below it.
    let prefix = git(dir, &["rev-parse", "--show-prefix"])?;
    let prefix = prefix.trim_end_matches('\n');
    let out = git(
        dir,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."],
    )?;
    let mut paths = BTreeSet::new();
    let mut entries = out.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let (status, path) = entry.split_at(entry.len().min(3));
        paths.insert(path.strip_prefix(prefix).unwrap_or(path).to_string());
        // A rename or copy is followed by the path it came from.
        if status.starts_with(['R', 'C']) {
            entries.next();
        }
    }
    Some(paths)
}
