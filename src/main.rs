mod akashic;
mod check;
mod claude;
mod design;
mod git;
mod history;
mod project;
mod session;
mod team;
mod text;

use std::ffi::OsString;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::project::Project;

/// Run a team of specialized Claude agents on a project, and improve the team
/// from every run.
///
/// A process belongs to a project, and has its own team: specialists and
/// skills that serve this process on this project. olympe keeps the teams of
/// a project and its akashic knowledge base in `~/.olympe/<project>/`.
///
/// `olympe design <PROCESS>` opens the workbench: a Claude session, the
/// designer, that settles with you what the process must achieve (its brief)
/// and the team that does it. Every change of a team is a commit in
/// `~/.olympe/<project>/`, which is a git repository.
///
/// `olympe <PROCESS>`, in the project's folder, starts a Claude session that
/// leads the process's team. When the session ends, the lead writes what the
/// run learned, which goes into the knowledge base, and what should change
/// in the team, which `olympe improve` applies.
#[derive(Debug, Parser)]
#[command(
    name = "olympe",
    version,
    propagate_version = true,
    override_usage = "olympe <PROCESS> [TASK]...\n       olympe <COMMAND>"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Make the current folder an olympe project: create ~/.olympe/<name>/,
    /// which holds its teams and its knowledge base
    Init {
        /// The project's name: lowercase letters, digits and hyphens.
        /// Default: from the folder's name
        #[arg(long)]
        name: Option<String>,
        /// Take this existing akashic knowledge base, by name (see
        /// `akashic kb list`) or folder. Default: create one in
        /// ~/.olympe/<name>/kb
        #[arg(long, value_name = "NAME|DIR")]
        kb: Option<String>,
    },
    /// Design a process and its team with Claude, in this terminal: its brief,
    /// its specialists, its skills and its playbook. On a process that
    /// exists, change them
    Design {
        /// The process's name: lowercase letters, digits and hyphens
        process: String,
        /// Have Claude write the team alone, with no session: from what a run
        /// of the process should achieve, or, for a process that exists, from
        /// the change you want
        #[arg(long, value_name = "REQUEST")]
        auto: Option<String>,
    },
    /// Run a process: start its lead session and its team in this terminal.
    /// `olympe <PROCESS>` is short for `olympe run <PROCESS>`
    Run(RunArgs),
    /// List the project's processes and their teams
    List,
    /// Check that a team can run and that its files agree with each other:
    /// one process, or every process of the project
    Check {
        /// The process to check
        process: Option<String>,
    },
    /// Have Claude change a process and its team from the feedback of its
    /// runs, within its brief
    Improve {
        /// The process to improve
        process: String,
    },
    #[command(external_subcommand)]
    Process(Vec<OsString>),
}

#[derive(Debug, Parser)]
#[command(name = "olympe", bin_name = "olympe", disable_version_flag = true)]
struct RunArgs {
    /// The process to run
    process: String,
    /// What this run works on, given to the lead
    task: Vec<String>,
    /// Do not ask the lead for a retrospective when the session ends
    #[arg(long)]
    no_retro: bool,
}

/// Exit codes: 0 success; 1 nothing to do, or `check` found something to
/// look at; 2 the command failed.
fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Init { name, kb } => project::init(name, kb)?,
        Command::Design { process, auto } => {
            if !design::design(&Project::find()?, &process, auto.as_deref())? {
                return Ok(ExitCode::from(1));
            }
        }
        Command::Check { process } => return check(&Project::find()?, process),
        Command::Run(args) => run_process(args)?,
        Command::Process(args) => {
            let args = std::iter::once(OsString::from("olympe")).chain(args);
            run_process(RunArgs::try_parse_from(args).unwrap_or_else(|err| err.exit()))?;
        }
        Command::List => {
            let project = Project::find()?;
            if !list(&project)? {
                eprintln!("no processes yet; `olympe design <process>` makes one");
                return Ok(ExitCode::from(1));
            }
        }
        Command::Improve { process } => {
            if !design::improve(&Project::find()?, &process)? {
                eprintln!(
                    "no feedback waits for `{process}`; a run of the process leaves some when \
                     its session ends"
                );
                return Ok(ExitCode::from(1));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_process(args: RunArgs) -> Result<()> {
    session::run(
        &Project::find()?,
        &args.process,
        &args.task.join(" "),
        !args.no_retro,
    )
}

/// Print what olympe finds in one process, or in every process.
fn check(project: &Project, process: Option<String>) -> Result<ExitCode> {
    let names = match process {
        Some(name) => vec![name],
        None => team::process_names(project)?,
    };
    if names.is_empty() {
        eprintln!("no processes yet; `olympe design <process>` makes one");
        return Ok(ExitCode::from(1));
    }
    let mut code = 0;
    for name in &names {
        match team::load(project, name) {
            Ok(team) => {
                let findings = check::findings(project, &team);
                if findings.is_empty() {
                    println!("{name}: can run, and nothing to look at");
                    continue;
                }
                println!("{name}: can run; to look at:");
                for finding in findings {
                    println!("  - {finding}");
                }
                code = code.max(1);
            }
            Err(err) => {
                println!("{name}: {err:#}");
                code = 2;
            }
        }
    }
    if code > 0 {
        eprintln!("`olympe design <process>` opens the designer on a process, with what olympe found");
    }
    Ok(ExitCode::from(code))
}

/// Print the processes. Returns false when there are none.
fn list(project: &Project) -> Result<bool> {
    let names = team::process_names(project)?;
    for name in &names {
        match team::load(project, name) {
            Ok(team) => {
                println!("{name}: {}", team.process.description);
                println!("  team: {}", team.specialist_names().join(", "));
                if !team.skills.is_empty() {
                    println!("  skills: {}", team.skill_names().join(", "));
                }
            }
            Err(err) => println!("{name}: {err:#}"),
        }
        let waiting = team::waiting_feedback(project, name)?.len();
        if waiting > 0 {
            println!("  the feedback of {waiting} run(s) waits: olympe improve {name}");
        }
    }
    Ok(!names.is_empty())
}
