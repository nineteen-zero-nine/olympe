//! The history of a project's teams: `~/.olympe/<project>/` is a git
//! repository, and each design session and each improvement is one commit, so
//! that a change of a team can be read and undone.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};

use crate::git;
use crate::project::KB_DIR;
use crate::team::{RUNS_DIR, SPECIALISTS_DIR};

/// The knowledge base has its own history, which akashic keeps. The records of
/// the runs are what happened, not what was designed: undoing a change of a
/// team must not take them away.
const IGNORED: [&str; 2] = [KB_DIR, RUNS_DIR];

/// What a commit made before any history existed is called.
const FIRST: &str = "what olympe kept before it kept a history";

fn is_repo(home: &Path) -> bool {
    home.join(".git").exists()
}

fn ensure_repo(home: &Path) -> Result<()> {
    if is_repo(home) {
        return Ok(());
    }
    git::run(home, &["init", "-q"])?;
    let ignore = home.join(".gitignore");
    if !ignore.exists() {
        let lines: String = IGNORED.iter().map(|dir| format!("/{dir}/\n")).collect();
        std::fs::write(&ignore, lines).with_context(|| format!("writing {}", ignore.display()))?;
    }
    Ok(())
}

/// Commit everything that changed in `home`. Returns the short name of the
/// commit, or None when nothing changed.
fn commit(home: &Path, message: &str) -> Result<Option<String>> {
    ensure_repo(home)?;
    git::run(home, &["add", "-A"])?;
    if git::succeeds(home, &["diff", "--cached", "--quiet"]) {
        return Ok(None);
    }
    // These commits are olympe's own record: they must not wait for a
    // signature, nor fail for want of an author.
    let mut args = vec!["-c", "commit.gpgsign=false"];
    if !git::succeeds(home, &["config", "user.name"]) {
        args.extend(["-c", "user.name=olympe"]);
    }
    if !git::succeeds(home, &["config", "user.email"]) {
        args.extend(["-c", "user.email=olympe@localhost"]);
    }
    args.extend(["commit", "-q", "-m", message]);
    git::run(home, &args)?;
    Ok(Some(git::run(home, &["rev-parse", "--short", "HEAD"])?.trim().to_string()))
}

pub enum Recorded {
    /// A commit, by its short name.
    Commit(String),
    /// Nothing had changed.
    Nothing,
    Failed,
}

/// Record the state of `home` under `message`. A history that cannot be
/// written is a warning: the work it would record is done.
pub fn record(home: &Path, message: &str) -> Recorded {
    match commit(home, message) {
        Ok(Some(commit)) => Recorded::Commit(commit),
        Ok(None) => Recorded::Nothing,
        Err(err) => {
            eprintln!("warning: this is not in the history of {}: {err:#}", home.display());
            Recorded::Failed
        }
    }
}

/// Before olympe has Claude change a team: record what changed since the last
/// commit, which olympe did not do, so that the next commit holds Claude's
/// changes and nothing else.
pub fn record_others(home: &Path) {
    let message = if is_repo(home) {
        "changes made outside olympe"
    } else {
        FIRST
    };
    record(home, message);
}

/// The specialists that `process` had at some point and has no more, by name.
/// Empty when there is no history.
pub fn former_specialists(home: &Path, process: &str, current: &[&str]) -> BTreeSet<String> {
    if !is_repo(home) {
        return BTreeSet::new();
    }
    let dir = format!("{process}/{SPECIALISTS_DIR}/");
    let Ok(log) = git::run(home, &["log", "--name-only", "--format=", "--", &dir]) else {
        return BTreeSet::new();
    };
    log.lines()
        .filter_map(|path| path.strip_prefix(&dir)?.strip_suffix(".md"))
        .filter(|name| !name.contains('/') && !current.contains(name))
        .map(str::to_string)
        .collect()
}
