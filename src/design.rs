//! The workbench: where a team is made and changed. `design` opens a Claude
//! session, the designer, that settles the process and its team with the
//! user, or writes them alone from one request; `improve` has the designer
//! change the team from the feedback of its runs. All of it happens in the
//! folder of the process, outside the project, and each change is one commit
//! in the history of the project's olympe folder.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::check;
use crate::claude;
use crate::git;
use crate::history::{self, Recorded};
use crate::project::Project;
use crate::team::{self, Team};

const DESIGNER: &str = include_str!("../templates/designer.md");
/// How the designer works with the user in the session.
const WORKSHOP: &str = include_str!("../templates/workshop.md");

/// File reading, searching and editing. No shell and no network, so nothing
/// the designer reads can make it run or send anything.
const TOOLS: &str = "Read,Write,Edit,Glob,Grep";

const NEW_SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string","description":"Two or three sentences: the team you designed and why it fits this project."}},"required":["summary"],"additionalProperties":false}"#;

const CHANGE_SCHEMA: &str = r#"{"type":"object","properties":{"changed":{"type":"boolean","description":"Whether you changed any file."},"summary":{"type":"string","description":"Each change and what it answers, and each thing asked for that you left out and why."}},"required":["changed","summary"],"additionalProperties":false}"#;

/// Claude reads the project and its knowledge base, and writes neither. This
/// remembers their state before a run, to report what changed. Only a folder
/// under git can be watched.
struct Watch {
    watched: Vec<(PathBuf, Option<BTreeSet<String>>)>,
}

impl Watch {
    fn start(project: &Project, kb: &Path) -> Watch {
        let watched = [project.root.as_path(), kb]
            .map(|dir| (dir.to_path_buf(), git::changed_paths(dir)))
            .to_vec();
        Watch { watched }
    }

    /// Warn about each file changed since `start`.
    fn report(&self) {
        let mut stray = Vec::new();
        for (dir, before) in &self.watched {
            let (Some(before), Some(after)) = (before, git::changed_paths(dir)) else {
                continue;
            };
            stray.extend(after.difference(before).map(|path| dir.join(path)));
        }
        if !stray.is_empty() {
            eprintln!("warning: Claude also changed files it was told to leave alone; check them:");
            for path in stray {
                eprintln!("  {}", path.display());
            }
        }
    }
}

fn bullets(lines: &[String], indent: &str) -> String {
    lines.iter().map(|line| format!("{indent}- {line}\n")).collect()
}

fn has_entries(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

/// The flags of a designer session: what it reads, its instructions, and
/// tools that write files and do nothing else. `reads` are the folders Claude
/// reads, which restricted mode would otherwise keep it out of.
fn designer_args(reads: &[&Path], instructions: &str) -> Vec<String> {
    // `--add-dir` takes every value up to the next flag.
    let mut args = vec!["--add-dir".to_string()];
    args.extend(reads.iter().map(|dir| dir.display().to_string()));
    args.extend(
        [
            "--append-system-prompt",
            instructions,
            "--restricted",
            "--tools",
            TOOLS,
            "--permission-mode",
            "acceptEdits",
        ]
        .map(String::from),
    );
    args
}

/// Have Claude write or change the definitions of `process` alone, in its
/// folder, then check them, giving Claude one chance to correct what olympe
/// finds. Fails when olympe cannot load the result. Returns Claude's
/// structured answer and the cost.
fn write_team(project: &Project, process: &str, prompt: &str, reads: &[&Path], schema: &str) -> Result<(Value, f64)> {
    let dir = team::process_dir(project, process);
    let args = |resume: Option<&str>| {
        let mut args = designer_args(reads, DESIGNER);
        args.extend(["--json-schema", schema, "--permission-prompts", "none"].map(String::from));
        if let Some(session) = resume {
            args.extend(["--resume".to_string(), session.to_string()]);
        }
        args
    };

    let reply = claude::headless(project, &dir, prompt, &args(None))?;
    let mut cost = reply.cost_usd;
    let mut problems = team::problems(project, process);
    let repair = if !problems.is_empty() {
        Some(format!(
            "olympe could not load what you wrote:\n{}\nCorrect the files.",
            bullets(&problems, "")
        ))
    } else {
        let findings = team::load(project, process)
            .map(|team| check::findings(project, &team))
            .unwrap_or_default();
        (!findings.is_empty()).then(|| {
            format!(
                "olympe checked the team and found:\n{}\nCorrect what is wrong, and leave the \
                 rest as it is.",
                bullets(&findings, "")
            )
        })
    };
    if let (Some(repair), Some(session)) = (repair, &reply.session_id) {
        cost += claude::headless(project, &dir, &repair, &args(Some(session)))?.cost_usd;
        problems = team::problems(project, process);
    }
    if !problems.is_empty() {
        bail!(
            "Claude wrote a process that olympe cannot load (cost ${cost:.2}). `olympe design \
             {process}` opens the designer on the files in {}, with what follows to correct:\n{}",
            dir.display(),
            bullets(&problems, "  ").trim_end()
        );
    }
    if reply.structured.is_null() {
        bail!("Claude finished without the report olympe asked for");
    }
    Ok((reply.structured, cost))
}

/// What the designer is told about where things are.
fn places(project: &Project, kb: &Path) -> String {
    format!(
        "The project `{}` is in `{}`. The pages of its knowledge base are in `{}/wiki/`.",
        project.name,
        project.root.display(),
        kb.display()
    )
}

/// The files of a team that loads, as the designer is told of them.
fn files(team: &Team) -> String {
    let skills = if team.skills.is_empty() {
        "It has no skill yet.".to_string()
    } else {
        let names: Vec<&str> = team.skills.iter().map(|skill| skill.name.as_str()).collect();
        format!("Its skills are in `{}/`: {}.", team::SKILLS_DIR, names.join(", "))
    };
    format!(
        "Its brief is `{}`, and the process is `{}`. Its specialists are in `{}/`: {}. {skills}",
        team::BRIEF_FILE,
        team::PROCESS_FILE,
        team::SPECIALISTS_DIR,
        team.specialist_names().join(", "),
    )
}

/// The feedback that waits for `process`, one file for each run, oldest
/// first, as a list of paths.
fn waiting_feedback(project: &Project, process: &str) -> Result<Vec<String>> {
    let runs_dir = team::runs_dir(project, process);
    Ok(team::waiting_feedback(project, process)?
        .iter()
        .map(|run| format!("`{}`", runs_dir.join(run).join(team::FEEDBACK_FILE).display()))
        .collect())
}

/// What a designer session is told before it starts: where things are, what
/// the folder of the process holds, what olympe finds wrong with it, the
/// feedback that waits, and the project's other processes.
fn situation(project: &Project, name: &str, kb: &Path) -> Result<String> {
    let dir = team::process_dir(project, name);
    let mut text = places(project, kb);
    text.push_str("\n\nThe current folder is the folder of the process. ");
    if !has_entries(&dir) {
        text.push_str("It is empty: the process does not exist yet.");
    } else {
        match team::load(project, name) {
            Ok(team) => {
                text.push_str(&files(&team));
                let findings = check::findings(project, &team);
                if !findings.is_empty() {
                    text.push_str(&format!(
                        "\n\nolympe checked the team and found:\n{}",
                        bullets(&findings, "").trim_end()
                    ));
                }
            }
            Err(_) => text.push_str(&format!(
                "It holds files that olympe cannot load as a process:\n{}",
                bullets(&team::problems(project, name), "").trim_end()
            )),
        }
    }

    let feedback = waiting_feedback(project, name)?;
    if !feedback.is_empty() {
        text.push_str(&format!(
            "\n\nThe feedback of {} run(s) of the process waits, one file for each run, oldest \
             first:\n{}\nRead it: it says how the team did. Do not change those files; `olympe \
             improve` is what uses them.",
            feedback.len(),
            bullets(&feedback, "")
        ));
    }

    let others: Vec<String> = team::process_names(project)?
        .into_iter()
        .filter(|other| other != name)
        .map(|other| match team::load(project, &other) {
            Ok(team) => format!(
                "`{other}`: {} Its specialists: {}.",
                team.process.description,
                team.specialist_names().join(", ")
            ),
            Err(_) => format!("`{other}`, which olympe cannot load."),
        })
        .collect();
    if others.is_empty() {
        text.push_str("\n\nThe project has no other process.");
    } else {
        text.push_str(&format!(
            "\n\nThe project's other processes, each with its own team, which this process shares \
             nothing with:\n{}",
            bullets(&others, "").trim_end()
        ));
    }
    Ok(text)
}

/// The folders a designer session reads.
fn reads(project: &Project, name: &str, kb: &Path) -> Result<Vec<PathBuf>> {
    let mut reads = vec![project.root.clone(), kb.to_path_buf()];
    if !team::waiting_feedback(project, name)?.is_empty() {
        reads.push(team::runs_dir(project, name));
    }
    Ok(reads)
}

/// Open the designer in this terminal, in the folder of the process, and wait
/// for the session to end.
fn workshop(project: &Project, name: &str, started: bool, kb: &Path) -> Result<()> {
    let opening = format!(
        "{} the `{name}` process and its team with me.\n\n{}",
        if started { "Revise" } else { "Design" },
        situation(project, name, kb)?
    );
    let reads = reads(project, name, kb)?;
    let reads: Vec<&Path> = reads.iter().map(PathBuf::as_path).collect();

    let bin = claude::bin(project);
    let mut cmd = Command::new(&bin);
    cmd.current_dir(team::process_dir(project, name))
        .args(["--name", &format!("{} design {name}", project.name)])
        .args(designer_args(&reads, &format!("{DESIGNER}{WORKSHOP}")));
    if let Some(model) = &project.config.claude.model {
        cmd.args(["--model", model]);
    }
    let status = cmd
        .arg(&opening)
        .status()
        .with_context(|| format!("could not run `{bin}`; is Claude Code installed?"))?;
    if !status.success() {
        eprintln!("warning: the session ended with {status}");
    }
    Ok(())
}

/// Have the designer write the process alone, from one request.
fn alone(project: &Project, name: &str, started: bool, request: &str, kb: &Path) -> Result<(Value, f64)> {
    let situation = situation(project, name, kb)?;
    let (prompt, schema) = if started {
        let prompt = format!(
            "Change the `{name}` process and its team.\n\n\
             What the user asks for: {request}\n\n\
             {situation}\n\n\
             - Read the brief, the process, its specialists and its skills first.\n\
             - Change what the request asks, in every file it touches, and keep the rest: do \
             not rewrite text that the request does not touch.\n\
             - Change the brief only where the request changes what the user decided.\n\
             - Nobody answers in this session. Settle from the project what the request leaves \
             open, and list it under `## Assumed` in the brief.\n\
             - Write in the current folder only."
        );
        (prompt, CHANGE_SCHEMA)
    } else {
        let prompt = format!(
            "Design the `{name}` process and its team.\n\n\
             What a run of the process should achieve: {request}\n\n\
             {situation}\n\n\
             Nobody answers in this session. Settle from the project what the user did not say, \
             and list it under `## Assumed` in the brief.\n\n\
             Write `{}`, `{}`, the file of each specialist in `{}/`, and each skill the team \
             needs in `{}/`, in the current folder.",
            team::BRIEF_FILE,
            team::PROCESS_FILE,
            team::SPECIALISTS_DIR,
            team::SKILLS_DIR
        );
        (prompt, NEW_SCHEMA)
    };
    let reads = reads(project, name, kb)?;
    let reads: Vec<&Path> = reads.iter().map(PathBuf::as_path).collect();
    eprintln!("Claude is reading the project and {} the team…", if started { "changing" } else { "designing" });
    write_team(project, name, &prompt, &reads, schema)
}

/// Create the process and its team, or change them: with the user in a
/// designer session, or, with `request`, by the designer alone. Returns false
/// when the session left nothing new.
pub fn design(project: &Project, name: &str, request: Option<&str>) -> Result<bool> {
    team::check_process_name(name)?;
    let dir = team::process_dir(project, name);
    let started = has_entries(&dir);
    let kb = project.kb_dir()?;

    history::record_others(&project.home);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let watch = Watch::start(project, &kb);
    let result = match request {
        Some(request) => alone(project, name, started, request, &kb).map(Some),
        None => workshop(project, name, started, &kb).map(|()| None),
    };
    watch.report();
    // When nothing was written, nothing is kept. This fails when the folder
    // holds a file.
    let _ = std::fs::remove_dir(&dir);
    if !dir.exists() {
        result?;
        eprintln!("nothing was written: the project has no process `{name}`");
        return Ok(false);
    }

    let problems = team::problems(project, name);
    let mut message = format!("design {name}");
    if !problems.is_empty() {
        message.push_str(" (cannot run yet)");
    }
    let summary = match &result {
        Ok(Some((report, _))) => report["summary"].as_str().map(|summary| summary.trim().to_string()),
        _ => None,
    };
    if let Some(summary) = &summary {
        message.push_str(&format!("\n\n{summary}"));
    }
    let recorded = history::record(&project.home, &message);
    let report = result?;
    if !problems.is_empty() {
        bail!(
            "the process `{name}` cannot run yet. `olympe design {name}` opens the designer on \
             the files in {}, with what follows to correct:\n{}",
            dir.display(),
            bullets(&problems, "  ").trim_end()
        );
    }

    let team = team::load(project, name)?;
    let unchanged = started && matches!(recorded, Recorded::Nothing);
    if unchanged {
        println!("the process `{name}` is as it was: nothing changed");
    } else {
        let verb = if started { "changed" } else { "created" };
        println!("{verb} the process `{name}`: {}", team.process.description);
        print_team(&team);
    }
    if let Some(summary) = summary {
        println!("\n{summary}");
    }
    print_findings(project, &team);
    println!();
    if let Some((_, cost)) = &report {
        print!("cost ${cost:.2}; ");
    }
    println!("the team is in {}", dir.display());
    print_recorded(project, &recorded);
    if !unchanged {
        println!("run it in the project's folder: olympe {name} [task]");
    }
    Ok(!unchanged)
}

fn print_team(team: &Team) {
    let width = team.specialists.iter().map(|a| a.name.len()).max().unwrap_or(0);
    for agent in &team.specialists {
        println!("  {:width$}  {}", agent.name, agent.description);
    }
    if !team.skills.is_empty() {
        println!("  skills: {}", team.skill_names().join(", "));
    }
}

fn print_findings(project: &Project, team: &Team) {
    let findings = check::findings(project, team);
    if !findings.is_empty() {
        println!(
            "\nto look at (`olympe design {}` opens the designer on these):\n{}",
            team.process.name,
            bullets(&findings, "  ").trim_end()
        );
    }
}

fn print_recorded(project: &Project, recorded: &Recorded) {
    if let Recorded::Commit(commit) = recorded {
        println!(
            "recorded as {commit} in the history of {} (a git repository)",
            project.home.display()
        );
    }
}

/// Returns false when no feedback waits for the process.
pub fn improve(project: &Project, name: &str) -> Result<bool> {
    let team = team::load(project, name)?;
    let runs = team::waiting_feedback(project, name)?;
    if runs.is_empty() {
        return Ok(false);
    }
    let dir = team::process_dir(project, name);
    let kb = project.kb_dir()?;
    let runs_dir = team::runs_dir(project, name);
    let prompt = format!(
        "Improve the `{name}` process and its team from the feedback of its runs.\n\n\
         The current folder is the folder of the process. {}\n\
         {}\n\n\
         The feedback, one file for each run, oldest first:\n{}\n\
         - Read the brief first, then every feedback file, then the process, its specialists and \
         its skills.\n\
         - The brief is what the user decided: every change must hold to it, and you do not \
         change `{}`. Leave out an item of feedback that asks for what the brief excludes, or \
         that only a change of the brief would answer, and say so in your summary: the user \
         changes the brief with `olympe design {name}`.\n\
         - Each change must answer an item of feedback. Change the playbook, a specialist's \
         instructions or a skill where the item points. Add, split or remove a specialist when \
         the feedback shows that the team lacks a craft or carries one it does not use. Write a \
         skill when the feedback shows a procedure that the team had to work out during a run.\n\
         - An item seen in several runs weighs more than an item seen once. Leave out an item \
         that one unusual run explains, or that contradicts the project's knowledge base.\n\
         - Keep what works: do not rewrite text that no feedback touches.\n\
         - Write in the current folder only. Do not change the feedback files.",
        files(&team),
        places(project, &kb),
        bullets(&waiting_feedback(project, name)?, ""),
        team::BRIEF_FILE,
    );

    history::record_others(&project.home);
    let brief_path = dir.join(team::BRIEF_FILE);
    let brief = std::fs::read_to_string(&brief_path).with_context(|| format!("reading {}", brief_path.display()))?;
    eprintln!("Claude is reading the feedback of {} run(s)…", runs.len());
    let watch = Watch::start(project, &kb);
    let result = write_team(project, name, &prompt, &[&project.root, &kb, &runs_dir], CHANGE_SCHEMA);
    watch.report();
    // The brief is the user's: an improvement does not change it.
    if std::fs::read_to_string(&brief_path).ok().as_deref() != Some(brief.as_str()) {
        std::fs::write(&brief_path, &brief).with_context(|| format!("restoring {}", brief_path.display()))?;
        eprintln!(
            "warning: Claude changed {}, which only you change; olympe put it back as it was",
            team::BRIEF_FILE
        );
    }

    let mut message = format!("improve {name}: the feedback of {} run(s)", runs.len());
    match &result {
        Ok((report, _)) => {
            if let Some(summary) = report["summary"].as_str() {
                message.push_str(&format!("\n\n{}", summary.trim()));
            }
        }
        Err(_) => message.push_str(" (cannot run yet)"),
    }
    let recorded = history::record(&project.home, &message);
    let (report, cost) = result?;

    // The feedback is used: keep it, under a name that no longer waits.
    for run in &runs {
        let from = team::run_dir(project, name, run).join(team::FEEDBACK_FILE);
        let to = from.with_file_name(team::APPLIED_FEEDBACK_FILE);
        std::fs::rename(&from, &to).with_context(|| format!("renaming {}", from.display()))?;
    }

    let changed = match recorded {
        Recorded::Commit(_) => true,
        Recorded::Nothing => false,
        Recorded::Failed => report["changed"].as_bool().unwrap_or(true),
    };
    if changed {
        println!("improved `{name}` from the feedback of {} run(s)", runs.len());
    } else {
        println!("left `{name}` as it is after reading the feedback of {} run(s)", runs.len());
    }
    if let Some(summary) = report["summary"].as_str() {
        println!("\n{}", summary.trim());
    }
    print_findings(project, &team::load(project, name)?);
    println!("\ncost ${cost:.2}; the team is in {}", dir.display());
    print_recorded(project, &recorded);
    Ok(true)
}
