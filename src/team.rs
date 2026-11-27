//! A process and its team, as the files of its folder define them:
//!
//! ```text
//! ~/.olympe/<project>/
//!   <process>/
//!     brief.md                 what the user decided: the process is held to it
//!     process.md               what a run achieves, and the lead's playbook
//!     specialists/<name>.md    one file for each specialist of the team
//!     skills/<name>/SKILL.md   the team's own skills
//!   runs/<process>/<stamp>/    what each run left: learnings.md, feedback.md
//! ```
//!
//! A specialist and a skill belong to one process, and to nothing else.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::project::{KB_DIR, Project};
use crate::text;

/// What `olympe <process>` cannot mean, since each is a command.
const COMMANDS: [&str; 7] = ["init", "design", "run", "list", "check", "improve", "help"];

pub const BRIEF_FILE: &str = "brief.md";
pub const PROCESS_FILE: &str = "process.md";
pub const SPECIALISTS_DIR: &str = "specialists";
pub const SKILLS_DIR: &str = "skills";
pub const SKILL_FILE: &str = "SKILL.md";
pub const RUNS_DIR: &str = "runs";
pub const FEEDBACK_FILE: &str = "feedback.md";
pub const LEARNINGS_FILE: &str = "learnings.md";
/// The name a run's feedback takes once `olympe improve` has used it.
pub const APPLIED_FEEDBACK_FILE: &str = "feedback.applied.md";

#[derive(Debug)]
pub struct Team {
    pub process: Process,
    /// What the user decided about the process. Only the user changes it.
    pub brief: String,
    /// At least one, sorted by name.
    pub specialists: Vec<Agent>,
    /// Sorted by name.
    pub skills: Vec<Skill>,
}

#[derive(Debug)]
pub struct Process {
    pub name: String,
    pub description: String,
    /// What the lead follows.
    pub playbook: String,
}

#[derive(Debug)]
pub struct Agent {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    /// The specialist's instructions.
    pub prompt: String,
}

#[derive(Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub procedure: String,
}

impl Team {
    pub fn specialist_names(&self) -> Vec<&str> {
        self.specialists.iter().map(|agent| agent.name.as_str()).collect()
    }

    /// The names the session knows the skills by: `<process>:<skill>`.
    pub fn skill_names(&self) -> Vec<String> {
        self.skills
            .iter()
            .map(|skill| format!("{}:{}", self.process.name, skill.name))
            .collect()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessMatter {
    description: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentMatter {
    description: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default, rename = "disallowedTools")]
    disallowed_tools: Option<Vec<String>>,
}

/// Claude Code reads a skill's file itself and knows more keys than these.
#[derive(Deserialize)]
struct SkillMatter {
    #[serde(default)]
    name: Option<String>,
    description: String,
}

/// The folder of a process.
pub fn process_dir(project: &Project, name: &str) -> PathBuf {
    project.home.join(name)
}

/// Fail unless `name` can name a process.
pub fn check_process_name(name: &str) -> Result<()> {
    if !text::is_name(name) {
        bail!("`{name}` is not a usable name: use lowercase letters, digits and hyphens only");
    }
    if COMMANDS.contains(&name) {
        bail!("`{name}` is an olympe command; give the process another name");
    }
    if name == KB_DIR {
        bail!("`{name}` is where the project's knowledge base is kept; give the process another name");
    }
    if name == RUNS_DIR {
        bail!("`{name}` is where the records of the runs are kept; give the process another name");
    }
    Ok(())
}

/// Split a file into its YAML front matter and the text after it.
fn split_front_matter(text: &str) -> Result<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .context("it does not start with a `---` line and front matter")?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    bail!("its front matter is not closed by a `---` line")
}

fn parse_process(name: &str, text: &str) -> Result<Process> {
    let (matter, body) = split_front_matter(text)?;
    let matter: ProcessMatter = serde_yaml_ng::from_str(matter).context("reading its front matter")?;
    if matter.description.trim().is_empty() {
        bail!("its `description` is empty");
    }
    if body.trim().is_empty() {
        bail!("it has no playbook after the front matter");
    }
    Ok(Process {
        name: name.to_string(),
        description: matter.description.trim().to_string(),
        playbook: body.trim().to_string(),
    })
}

fn parse_agent(name: &str, text: &str) -> Result<Agent> {
    let (matter, body) = split_front_matter(text)?;
    let matter: AgentMatter = serde_yaml_ng::from_str(matter).context("reading its front matter")?;
    if matter.description.trim().is_empty() {
        bail!("its `description` is empty");
    }
    if body.trim().is_empty() {
        bail!("it has no instructions after the front matter");
    }
    Ok(Agent {
        name: name.to_string(),
        description: matter.description.trim().to_string(),
        model: matter.model,
        tools: matter.tools,
        disallowed_tools: matter.disallowed_tools,
        prompt: body.trim().to_string(),
    })
}

fn parse_skill(name: &str, text: &str) -> Result<Skill> {
    let (matter, body) = split_front_matter(text)?;
    let matter: SkillMatter = serde_yaml_ng::from_str(matter).context("reading its front matter")?;
    if let Some(given) = &matter.name
        && given != name
    {
        bail!("its `name` is `{given}`, and its folder is `{name}`: they must be the same");
    }
    if matter.description.trim().is_empty() {
        bail!("its `description` is empty");
    }
    if body.trim().is_empty() {
        bail!("it has no procedure after the front matter");
    }
    Ok(Skill {
        name: name.to_string(),
        description: matter.description.split_whitespace().collect::<Vec<_>>().join(" "),
        procedure: body.trim().to_string(),
    })
}

/// The entries of a folder as (name, path), sorted. None when the folder
/// does not exist.
fn entries(dir: &Path) -> Result<Option<Vec<(String, PathBuf)>>> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("reading {}", dir.display())),
    };
    let mut entries = Vec::new();
    for entry in read {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        entries.push((entry.file_name().to_string_lossy().into_owned(), entry.path()));
    }
    entries.sort();
    Ok(Some(entries))
}

/// Read one definition file. `what` names it relative to the process folder,
/// which is how Claude and the user both find it.
fn read<T>(path: &Path, what: &str, parse: impl FnOnce(&str) -> Result<T>) -> Result<T> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => bail!("{what} does not exist"),
        Err(err) => return Err(err).with_context(|| format!("reading {what}")),
    };
    parse(&text).with_context(|| format!("{what} cannot be used"))
}

/// Read every definition in the folder of a process. Returns the team, or
/// everything that keeps it from running, one line each.
fn scan(dir: &Path, name: &str) -> std::result::Result<Team, Vec<String>> {
    let mut problems = Vec::new();
    let mut keep = |result: Result<()>| {
        if let Err(err) = result {
            problems.push(format!("{err:#}"));
        }
    };

    let mut process = None;
    keep(read(&dir.join(PROCESS_FILE), PROCESS_FILE, |text| parse_process(name, text)).map(|p| process = Some(p)));

    let mut brief = String::new();
    keep(
        read(&dir.join(BRIEF_FILE), BRIEF_FILE, |text| match text.trim() {
            "" => bail!("it is empty"),
            text => Ok(text.to_string()),
        })
        .map(|text| brief = text),
    );

    let mut specialists = Vec::new();
    let mut files = 0;
    match entries(&dir.join(SPECIALISTS_DIR)) {
        Err(err) => keep(Err(err)),
        Ok(found) => {
            for (file, path) in found.unwrap_or_default() {
                let Some(stem) = file.strip_suffix(".md") else {
                    continue;
                };
                files += 1;
                let what = format!("{SPECIALISTS_DIR}/{file}");
                if !text::is_name(stem) {
                    keep(Err(anyhow::anyhow!(
                        "{what}: a specialist's name has lowercase letters, digits and hyphens only"
                    )));
                    continue;
                }
                keep(read(&path, &what, |text| parse_agent(stem, text)).map(|agent| specialists.push(agent)));
            }
        }
    }
    if files == 0 {
        keep(Err(anyhow::anyhow!(
            "{SPECIALISTS_DIR}/ holds no specialist: a process needs at least one"
        )));
    }

    let mut skills = Vec::new();
    match entries(&dir.join(SKILLS_DIR)) {
        Err(err) => keep(Err(err)),
        Ok(found) => {
            for (folder, path) in found.unwrap_or_default() {
                if !path.is_dir() {
                    continue;
                }
                let what = format!("{SKILLS_DIR}/{folder}/{SKILL_FILE}");
                if !text::is_name(&folder) {
                    keep(Err(anyhow::anyhow!(
                        "{SKILLS_DIR}/{folder}: a skill's name has lowercase letters, digits and hyphens only"
                    )));
                    continue;
                }
                keep(read(&path.join(SKILL_FILE), &what, |text| parse_skill(&folder, text)).map(|skill| skills.push(skill)));
            }
        }
    }

    match process {
        Some(process) if problems.is_empty() => Ok(Team {
            process,
            brief,
            specialists,
            skills,
        }),
        _ => Err(problems),
    }
}

/// The process and its team.
pub fn load(project: &Project, name: &str) -> Result<Team> {
    check_process_name(name)?;
    let dir = process_dir(project, name);
    if !dir.join(PROCESS_FILE).is_file() {
        bail!(
            "the project `{}` has no process named `{name}`: `olympe list` shows its processes, \
             and `olympe design {name}` creates this one",
            project.name
        );
    }
    scan(&dir, name).map_err(|problems| {
        anyhow::anyhow!(
            "the process `{name}` in {} cannot run:\n{}",
            dir.display(),
            problems.iter().map(|p| format!("  - {p}\n")).collect::<String>().trim_end()
        )
    })
}

/// Everything that keeps the process from running, one line each. Empty when
/// it can run.
pub fn problems(project: &Project, name: &str) -> Vec<String> {
    scan(&process_dir(project, name), name).err().unwrap_or_default()
}

/// The project's processes: each folder of its olympe folder that holds a
/// `process.md`.
pub fn process_names(project: &Project) -> Result<Vec<String>> {
    Ok(entries(&project.home)?
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, path)| check_process_name(name).is_ok() && path.join(PROCESS_FILE).is_file())
        .map(|(name, _)| name)
        .collect())
}

/// Where the runs of `process` leave their records. This is outside the
/// folder of the process: Claude Code loads that folder as a plugin, for its
/// skills, and lets a session write nothing inside a plugin.
pub fn runs_dir(project: &Project, process: &str) -> PathBuf {
    project.home.join(RUNS_DIR).join(process)
}

/// The folder of the run of `process` that started at `stamp`.
pub fn run_dir(project: &Project, process: &str, stamp: &str) -> PathBuf {
    runs_dir(project, process).join(stamp)
}

/// The runs of `process` whose feedback `olympe improve` has not used yet, by
/// the name of their folder, oldest first.
pub fn waiting_feedback(project: &Project, process: &str) -> Result<Vec<String>> {
    Ok(entries(&runs_dir(project, process))?
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, path)| path.join(FEEDBACK_FILE).is_file())
        .map(|(run, _)| run)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_front_matter() {
        let (matter, body) = split_front_matter("---\na: 1\n---\nbody\n---\nmore\n").unwrap();
        assert_eq!(matter, "a: 1\n");
        assert_eq!(body, "body\n---\nmore\n");
        assert!(split_front_matter("no front matter").is_err());
        assert!(split_front_matter("---\na: 1\n").is_err());
    }

    #[test]
    fn parses_a_process() {
        let process = parse_process("review", "---\ndescription: Review a change.\n---\n\nDo it.\n").unwrap();
        assert_eq!(process.description, "Review a change.");
        assert_eq!(process.playbook, "Do it.");
    }

    #[test]
    fn refuses_a_process_it_cannot_run() {
        let parse = |text| parse_process("review", text).unwrap_err().to_string();
        assert!(parse("---\ndescription: ''\n---\nDo it.\n").contains("is empty"));
        assert!(parse("---\ndescription: x\n---\n").contains("no playbook"));
        // A specialist is a file of the process, not a name in a list.
        assert!(parse_process("review", "---\ndescription: x\nteam: [a]\n---\nDo.\n").is_err());
    }

    #[test]
    fn parses_an_agent() {
        let agent = parse_agent(
            "reader",
            "---\ndescription: Reads.\nmodel: sonnet\ndisallowedTools: [Edit, Write]\n---\nYou read.\n",
        )
        .unwrap();
        assert_eq!(agent.model.as_deref(), Some("sonnet"));
        assert_eq!(agent.disallowed_tools.unwrap(), ["Edit", "Write"]);
        assert!(agent.tools.is_none());
    }

    #[test]
    fn parses_a_skill() {
        let skill = parse_skill(
            "tone-check",
            "---\nname: tone-check\ndescription: Check a text\n  against the tone rules.\nallowed-tools: Read\n---\nRead it.\n",
        )
        .unwrap();
        assert_eq!(skill.description, "Check a text against the tone rules.");
        let err = parse_skill("tone-check", "---\nname: tone\ndescription: x\n---\nRead it.\n").unwrap_err();
        assert!(err.to_string().contains("must be the same"));
        assert!(parse_skill("tone-check", "---\nname: tone-check\n---\nRead it.\n").is_err());
    }

    #[test]
    fn a_process_cannot_take_a_name_olympe_uses() {
        assert!(check_process_name("code-review").is_ok());
        for name in ["list", "kb", "runs", "Review", ""] {
            assert!(check_process_name(name).is_err(), "{name}");
        }
    }
}
