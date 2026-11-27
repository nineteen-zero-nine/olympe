//! A run: the lead session with its team, in the project's folder, then the
//! retrospective that feeds the knowledge base and the next improvement of
//! the team.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use crate::akashic;
use crate::claude;
use crate::project::Project;
use crate::team::{self, Team};
use crate::text::{self, fill};

const LEAD: &str = include_str!("../templates/lead.md");
const TEAMMATE: &str = include_str!("../templates/teammate.md");
const KB: &str = include_str!("../templates/kb.md");
const RETRO: &str = include_str!("../templates/retro.md");

/// The flags that make a Claude session the lead of the process's team: the
/// specialists as agents it can start, the team's skills, and its own
/// instructions. With `run_dir`, the session can also write there.
fn team_args(project: &Project, team: &Team, kb: &Path, run_dir: Option<&Path>) -> Vec<String> {
    let process = &team.process;
    let kb = fill(KB, &[("kb", &kb.display().to_string())]);
    let about = [
        ("process", process.name.as_str()),
        ("project", project.name.as_str()),
        ("kb", kb.trim_end()),
    ];

    let mut definitions = Map::new();
    let mut roster = String::new();
    for agent in &team.specialists {
        let mut values = about.to_vec();
        values.push(("name", &agent.name));
        let mut definition = json!({
            "description": agent.description,
            "prompt": format!("{}\n{}", agent.prompt, fill(TEAMMATE, &values)),
        });
        if let Some(model) = &agent.model {
            definition["model"] = json!(model);
        }
        if let Some(tools) = &agent.tools {
            definition["tools"] = json!(tools);
        }
        if let Some(tools) = &agent.disallowed_tools {
            definition["disallowedTools"] = json!(tools);
        }
        definitions.insert(agent.name.clone(), definition);
        roster.push_str(&format!("- `{}`: {}\n", agent.name, agent.description));
    }

    let mut skills = String::new();
    if !team.skills.is_empty() {
        skills.push_str(
            "\n## The team's skills\n\n\
             Procedures of this project, which you and every specialist use with the Skill tool \
             when the playbook or a specialist's instructions call for one.\n\n",
        );
        for (skill, name) in team.skills.iter().zip(team.skill_names()) {
            skills.push_str(&format!("- `{name}`: {}\n", skill.description));
        }
    }

    let root = project.root.display().to_string();
    let mut values = about.to_vec();
    values.extend([
        ("root", root.as_str()),
        ("roster", roster.trim_end()),
        ("skills", skills.as_str()),
        ("brief", team.brief.as_str()),
        ("playbook", process.playbook.as_str()),
    ]);

    // `--add-dir` takes every value up to the next flag, so a flag follows it.
    let mut args = Vec::new();
    if let Some(dir) = run_dir {
        args.extend(["--add-dir".to_string(), dir.display().to_string()]);
    }
    if !team.skills.is_empty() {
        // Claude Code loads the folder of the process as a plugin named after
        // it, which gives the lead and every specialist its `skills/`.
        let dir = team::process_dir(project, &process.name);
        args.extend(["--plugin-dir".to_string(), dir.display().to_string()]);
    }
    args.extend([
        "--agents".to_string(),
        Value::Object(definitions).to_string(),
        "--append-system-prompt".to_string(),
        fill(LEAD, &values),
    ]);
    args
}

/// Start the lead session of the process in this terminal, in the project's
/// folder, and wait for it to end; then, with `retro`, close the run with its
/// retrospective.
pub fn run(project: &Project, name: &str, task: &str, retro: bool) -> Result<()> {
    let team = team::load(project, name)?;
    let kb = project.kb_dir()?;
    // The retrospective is written outside the project, in the run's folder,
    // which the session must be allowed to write to. It is outside the folder
    // of the process too, where Claude Code lets a session write nothing once
    // that folder is loaded as a plugin.
    let run_dir = retro.then(|| team::run_dir(project, name, &text::now().stamp));
    if let Some(dir) = &run_dir {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let team_args = team_args(project, &team, &kb, run_dir.as_deref());
    let session = uuid::Uuid::new_v4().to_string();
    let opening = match task.trim() {
        "" => format!(
            "Run the `{name}` process. I gave no task: if the playbook needs one, ask me for it."
        ),
        task => format!("Run the `{name}` process.\n\nThe task: {task}"),
    };

    let bin = claude::bin(project);
    let mut cmd = Command::new(&bin);
    cmd.current_dir(&project.root)
        .args(["--session-id", &session])
        .args(["--name", &format!("{} {name}", project.name)])
        .args(&team_args);
    if let Some(model) = &project.config.claude.model {
        cmd.args(["--model", model]);
    }
    cmd.arg(&opening)
        // A lead needs teammates, which Claude Code keeps behind this switch.
        .env("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS", "1")
        .env("AKASHIC_KB", &kb);
    let status = cmd
        .status()
        .with_context(|| format!("could not run `{bin}`; is Claude Code installed?"));

    let Some(run_dir) = run_dir else {
        status?;
        return Ok(());
    };
    let result = status.map(|status| {
        if !status.success() {
            eprintln!("warning: the session ended with {status}");
        }
        if let Err(err) = retrospective(project, name, &session, &team_args, &kb, &run_dir) {
            eprintln!(
                "warning: no retrospective for this run: {err:#}\n\
                 The session is kept: `claude --resume {session}` opens it again."
            );
        }
    });
    // A run that left nothing keeps no folder. This fails when it holds a
    // file, which is the case to keep it.
    let _ = std::fs::remove_dir(&run_dir);
    result
}

/// Have the lead write what the run learned and what should change in the
/// team, then record the first in the knowledge base. The second waits for
/// `olympe improve`.
fn retrospective(
    project: &Project,
    name: &str,
    session: &str,
    team_args: &[String],
    kb: &Path,
    run_dir: &Path,
) -> Result<()> {
    let date = text::now().date;
    let dir = run_dir.display().to_string();
    let prompt = fill(RETRO, &[("process", name), ("dir", &dir), ("date", &date)]);

    // The same team flags as the session, so that the lead continues as it was.
    let mut args = vec!["--resume".to_string(), session.to_string()];
    args.extend_from_slice(team_args);
    args.extend(
        ["--permission-mode", "acceptEdits", "--permission-prompts", "none"].map(String::from),
    );
    eprintln!("the session is over; the lead is writing the run's retrospective…");
    let reply = claude::headless(project, &project.root, &prompt, &args)?;

    let written = |file: &str| {
        let path = run_dir.join(file);
        std::fs::read_to_string(&path)
            .is_ok_and(|text| !text.trim().is_empty())
            .then_some(path)
    };
    let learnings = written(team::LEARNINGS_FILE);
    let feedback = written(team::FEEDBACK_FILE);
    if learnings.is_none() && feedback.is_none() {
        println!("the lead found nothing to record from this run (cost ${:.2})", reply.cost_usd);
        return Ok(());
    }
    println!("retrospective written in {dir}/ (cost ${:.2})", reply.cost_usd);

    if let Some(file) = learnings {
        let title = format!("{name} run {date}");
        println!("recording {} in the knowledge base…", team::LEARNINGS_FILE);
        if let Err(err) = akashic::ingest(kb, &file, &title) {
            eprintln!(
                "warning: the learnings are not in the knowledge base yet: {err:#}\n\
                 Record them with: {}",
                akashic::ingest_command(kb, &file, &title)
            );
        }
    }
    if feedback.is_some() {
        let waiting = team::waiting_feedback(project, name)?.len();
        println!(
            "the feedback of {waiting} run(s) waits; `olympe improve {name}` changes the \
             process and its team from it"
        );
    }
    Ok(())
}
