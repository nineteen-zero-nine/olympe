use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::project::Project;
use crate::text::excerpt;

/// The Claude Code binary: `$OLYMPE_CLAUDE_BIN`, else the project's setting,
/// else `claude` from the PATH.
pub fn bin(project: &Project) -> String {
    std::env::var("OLYMPE_CLAUDE_BIN")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| project.config.claude.bin.clone())
        .unwrap_or_else(|| "claude".into())
}

pub struct Reply {
    pub session_id: Option<String>,
    pub cost_usd: f64,
    /// The answer shaped by `--json-schema`, or null when none was asked for.
    pub structured: Value,
}

/// Run Claude Code once, without a terminal, in `dir`, and wait for its
/// answer. `args` adds to `-p <prompt> --output-format json`.
pub fn headless(project: &Project, dir: &Path, prompt: &str, args: &[String]) -> Result<Reply> {
    let bin = bin(project);
    let mut cmd = Command::new(&bin);
    cmd.current_dir(dir)
        .arg("-p")
        .arg(prompt)
        .args(["--output-format", "json"])
        .args(args);
    if let Some(model) = &project.config.claude.model {
        cmd.args(["--model", model]);
    }
    if let Some(budget) = project.config.claude.max_budget_usd {
        cmd.args(["--max-budget-usd", &budget.to_string()]);
    }

    let output = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("could not run `{bin}`; is Claude Code installed?"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let Ok(result) = serde_json::from_str::<Value>(stdout.trim()) else {
        bail!(
            "`{bin}` did not return JSON ({}): {}",
            output.status,
            excerpt(if stderr.trim().is_empty() { &stdout } else { &stderr }, 600)
        );
    };

    let text = result["result"].as_str().unwrap_or_default();
    if !output.status.success() || result["is_error"].as_bool().unwrap_or(false) {
        let reason = if text.is_empty() {
            result["subtype"].as_str().unwrap_or("unknown failure")
        } else {
            text
        };
        bail!("the Claude run failed: {}", excerpt(reason, 600));
    }

    Ok(Reply {
        session_id: result["session_id"].as_str().map(str::to_string),
        cost_usd: result["total_cost_usd"].as_f64().unwrap_or(0.0),
        structured: result["structured_output"].clone(),
    })
}
