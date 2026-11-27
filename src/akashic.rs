//! The project's knowledge base is an akashic knowledge base. olympe reaches
//! it through the `akashic` command only.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The file that marks the root of a knowledge base.
pub const CONFIG_FILE: &str = "akashic.toml";

fn bin() -> String {
    std::env::var("OLYMPE_AKASHIC_BIN")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "akashic".into())
}

fn command() -> Command {
    let mut cmd = Command::new(bin());
    // The knowledge base is always passed with --kb.
    cmd.env_remove("AKASHIC_KB");
    cmd
}

fn not_found(err: std::io::Error) -> anyhow::Error {
    anyhow::Error::new(err).context(format!("could not run `{}`; is akashic installed?", bin()))
}

/// Create a knowledge base in `dir` and give it `name`. akashic prints what
/// it did, which includes the GitHub repository it creates when the user's
/// akashic configuration asks for one.
pub fn init(dir: &Path, name: &str) -> Result<()> {
    let status = command()
        .arg("init")
        .arg(dir)
        .args(["--name", name])
        .stdin(Stdio::null())
        .status()
        .map_err(not_found)?;
    if !status.success() {
        bail!(
            "`akashic init` could not create the knowledge base; to use an existing one, \
             pass --kb <name or folder>"
        );
    }
    Ok(())
}

#[derive(Deserialize)]
struct Named {
    name: String,
    path: PathBuf,
}

/// The folder of the knowledge base that akashic knows as `name`.
pub fn named(name: &str) -> Result<Option<PathBuf>> {
    let out = command()
        .args(["kb", "list", "--json"])
        .stdin(Stdio::null())
        .output()
        .map_err(not_found)?;
    if !out.status.success() {
        bail!(
            "`akashic kb list` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let list: Vec<Named> =
        serde_json::from_slice(&out.stdout).context("reading the output of `akashic kb list`")?;
    Ok(list.into_iter().find(|kb| kb.name == name).map(|kb| kb.path))
}

/// The command that records `file` in the knowledge base at `kb`.
pub fn ingest_command(kb: &Path, file: &Path, title: &str) -> String {
    format!(
        "akashic ingest '{}' --title '{title}' --kb '{}'",
        file.display(),
        kb.display()
    )
}

/// Record `file` in the knowledge base at `kb`. This is one Claude run, and
/// akashic prints what it wrote.
pub fn ingest(kb: &Path, file: &Path, title: &str) -> Result<()> {
    let status = command()
        .arg("ingest")
        .arg(file)
        .args(["--title", title])
        .arg("--kb")
        .arg(kb)
        .stdin(Stdio::null())
        .status()
        .map_err(not_found)?;
    if !status.success() {
        bail!("`akashic ingest` failed");
    }
    Ok(())
}
