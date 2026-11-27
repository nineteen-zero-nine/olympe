#![cfg(unix)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// A scratch folder holding a project's folder (`project/`), the olympe home
/// (`home/`), and the logs of the fake Claude and the fake akashic.
struct Fixture {
    dir: TempDir,
}

const BRIEF: &str = "# review\n\n## Goal\nNo defect reaches the main branch.\n";
const REVIEW: &str = "---\ndescription: Review a change before it merges.\n---\n\
                      The reader reads the diff, then the tester runs the suite.\n";
const READER: &str = "---\ndescription: Reads a diff for defects.\ndisallowedTools: [Edit, Write]\n---\n\
                      You read diffs.\n";
const TESTER: &str = "---\ndescription: Runs the test suite.\nmodel: sonnet\n---\nYou run `cargo test`.\n";

impl Fixture {
    /// A fixture whose `project` folder is not a project yet.
    fn bare() -> Fixture {
        let fixture = Fixture {
            dir: TempDir::new().expect("temp dir"),
        };
        std::fs::create_dir(fixture.project()).unwrap();
        fixture
    }

    /// A fixture with an initialised project.
    fn new() -> Fixture {
        let fixture = Fixture::bare();
        fixture.olympe().arg("init").assert().success();
        fixture
    }

    /// A fixture with a project that has the `review` process, written by hand.
    fn with_review() -> Fixture {
        let fixture = Fixture::new();
        fixture.write("review/brief.md", BRIEF);
        fixture.write("review/process.md", REVIEW);
        fixture.write("review/specialists/reader.md", READER);
        fixture.write("review/specialists/tester.md", TESTER);
        fixture
    }

    /// The scratch folder, as the path olympe resolves it to.
    fn root(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    /// The project's folder: where its teams work.
    fn project(&self) -> PathBuf {
        self.root().join("project")
    }

    /// What olympe keeps for the project: its teams and its knowledge base.
    fn home(&self) -> PathBuf {
        self.root().join("home/project")
    }

    /// Write a file in the project's olympe folder.
    fn write(&self, rel: &str, content: &str) {
        let path = self.home().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.home().join(rel)).unwrap()
    }

    fn exists(&self, rel: &str) -> bool {
        self.home().join(rel).exists()
    }

    /// The names of what the project's folder holds, sorted.
    fn project_files(&self) -> Vec<String> {
        names_in(&self.project())
    }

    fn claude_log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("claude.log")).unwrap_or_default()
    }

    /// Every call of the fake Claude so far: the folder it ran in, and its
    /// arguments.
    fn claude_calls(&self) -> Vec<(PathBuf, String)> {
        self.claude_log()
            .split("=== call in ")
            .filter(|call| !call.is_empty())
            .map(|call| {
                let (cwd, args) = call.split_once('\n').expect("a folder, then the arguments");
                (PathBuf::from(cwd), args.to_string())
            })
            .collect()
    }

    fn akashic_log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("akashic.log")).unwrap_or_default()
    }

    /// The folders of the runs of a process, sorted.
    fn runs(&self, process: &str) -> Vec<String> {
        names_in(&self.home().join("runs").join(process))
    }

    /// olympe running in the project's folder, with the fake Claude and the
    /// fake akashic.
    fn olympe(&self) -> Command {
        let mut cmd = Command::cargo_bin("olympe").expect("binary builds");
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        cmd.current_dir(self.project())
            .env("OLYMPE_HOME", self.root().join("home"))
            .env("OLYMPE_CLAUDE_BIN", tests.join("fake-claude.sh"))
            .env("OLYMPE_AKASHIC_BIN", tests.join("fake-akashic.sh"))
            .env("FAKE_LOG", self.dir.path().join("claude.log"))
            .env("FAKE_AKASHIC_LOG", self.dir.path().join("akashic.log"))
            .env_remove("FAKE_KBS")
            .env_remove("AKASHIC_KB");
        isolate_git(&mut cmd);
        cmd
    }

    fn git(&self, args: &[&str]) {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(self.project()).args(args);
        isolate_git(&mut cmd);
        assert!(cmd.output().expect("git runs").status.success(), "git {args:?} failed");
    }

    /// What git prints for `args` in the project's olympe folder.
    fn home_git(&self, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(self.home()).args(args);
        isolate_git(&mut cmd);
        let out = cmd.output().expect("git runs");
        assert!(out.status.success(), "git {args:?} failed");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// The history of the project's teams: the first line of each commit,
    /// newest first.
    fn history(&self) -> Vec<String> {
        self.home_git(&["log", "--format=%s"]).lines().map(str::to_string).collect()
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn isolate_git(cmd: &mut Command) {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid");
}

/// The value that follows `flag` in a logged call.
fn value_after<'a>(call: &'a str, flag: &str) -> &'a str {
    let rest = &call[call.find(flag).unwrap_or_else(|| panic!("no {flag} in {call}")) + flag.len()..];
    rest.split_whitespace().next().expect("a value")
}

#[test]
fn no_subcommand_prints_usage_and_fails() {
    Command::cargo_bin("olympe")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
}

// init

#[test]
fn init_creates_the_olympe_folder_of_the_project_and_its_knowledge_base() {
    let fixture = Fixture::bare();
    fixture
        .olympe()
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("`project` is now an olympe project"))
        .stdout(predicate::str::contains(fixture.home().display().to_string()));

    let config = fixture.read("olympe.toml");
    assert!(config.contains(&format!("root = \"{}\"", fixture.project().display())), "{config}");
    assert!(config.contains("kb = \"kb\""), "{config}");
    let log = fixture.akashic_log();
    assert_eq!(log.trim_end(), format!("init {} --name project", fixture.home().join("kb").display()));
    // olympe writes nothing in the project's folder.
    assert_eq!(fixture.project_files(), Vec::<String>::new());
}

#[test]
fn the_olympe_home_is_dot_olympe_in_the_home_folder() {
    let fixture = Fixture::bare();
    let user_home = fixture.root().join("user");
    fixture
        .olympe()
        .env_remove("OLYMPE_HOME")
        .env("HOME", &user_home)
        .arg("init")
        .assert()
        .success();
    assert!(user_home.join(".olympe/project/olympe.toml").is_file());
}

#[test]
fn init_takes_a_name() {
    let fixture = Fixture::bare();
    fixture.olympe().args(["init", "--name", "shop"]).assert().success();
    assert!(fixture.root().join("home/shop/olympe.toml").is_file());
    assert!(fixture.akashic_log().contains("--name shop"));

    Fixture::bare()
        .olympe()
        .args(["init", "--name", "My Shop"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a usable name"));
}

#[test]
fn init_refuses_a_folder_or_a_name_that_is_a_project() {
    let fixture = Fixture::new();
    fixture
        .olympe()
        .args(["init", "--name", "again"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("already an olympe project: `project`"));

    let other = fixture.root().join("other");
    std::fs::create_dir(&other).unwrap();
    fixture
        .olympe()
        .current_dir(&other)
        .args(["init", "--name", "project"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("the name `project` is taken by the project in"));
    fixture.olympe().current_dir(&other).arg("init").assert().success();
}

#[test]
fn init_keeps_nothing_when_the_knowledge_base_cannot_be_created() {
    let fixture = Fixture::bare();
    fixture
        .olympe()
        .env("FAKE_AKASHIC", "fail-init")
        .arg("init")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("pass --kb"));
    assert!(!fixture.home().exists());
}

#[test]
fn init_takes_an_existing_knowledge_base_folder_for_one_project_only() {
    let fixture = Fixture::bare();
    let kb = fixture.root().join("shared-kb");
    std::fs::create_dir(&kb).unwrap();
    std::fs::write(kb.join("akashic.toml"), "").unwrap();

    fixture.olympe().args(["init", "--kb", "../shared-kb"]).assert().success();
    let config = fixture.read("olympe.toml");
    assert!(config.contains(&format!("kb = \"{}\"", kb.display())), "{config}");
    assert!(!fixture.akashic_log().contains("init"));
    assert!(!fixture.exists("kb"));

    // A project has one knowledge base, and a knowledge base one project.
    let other = fixture.root().join("other");
    std::fs::create_dir(&other).unwrap();
    fixture
        .olympe()
        .current_dir(&other)
        .args(["init", "--kb", "../shared-kb"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("is the knowledge base of the project `project`"));
    assert!(!fixture.root().join("home/other").exists());
}

#[test]
fn init_takes_a_knowledge_base_akashic_knows_by_name() {
    let fixture = Fixture::bare();
    let kb = fixture.root().join("shared-kb");
    std::fs::create_dir(&kb).unwrap();
    std::fs::write(kb.join("akashic.toml"), "").unwrap();
    let kbs = format!(r#"[{{"name":"shared","path":"{}","usable":true}}]"#, kb.display());

    fixture
        .olympe()
        .env("FAKE_KBS", &kbs)
        .args(["init", "--kb", "shared"])
        .assert()
        .success()
        .stdout(predicate::str::contains("shared-kb"));
    assert!(fixture.read("olympe.toml").contains("kb = \"shared\""));

    // Without the name, the project's commands say what is missing.
    fixture.write("review/brief.md", BRIEF);
    fixture.write("review/process.md", REVIEW);
    fixture.write("review/specialists/reader.md", READER);
    fixture
        .olympe()
        .args(["review", "--no-retro"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("finding the knowledge base `shared`"));
    fixture
        .olympe()
        .env("FAKE_KBS", &kbs)
        .args(["review", "--no-retro"])
        .assert()
        .success();
}

#[test]
fn init_refuses_a_knowledge_base_it_cannot_find() {
    let fixture = Fixture::bare();
    fixture
        .olympe()
        .args(["init", "--kb", "nowhere"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("neither a knowledge base folder nor a name"));
    assert!(!fixture.home().exists());
}

// finding the project

#[test]
fn commands_need_a_project() {
    let fixture = Fixture::bare();
    for args in [&["list"][..], &["review"], &["design", "review"], &["check"], &["improve", "review"]] {
        fixture
            .olympe()
            .args(args)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("no olympe project found"))
            .stderr(predicate::str::contains("olympe init"));
    }
    assert_eq!(fixture.claude_log(), "");

    // Outside every project, olympe says which projects it knows.
    let fixture = Fixture::with_review();
    fixture
        .olympe()
        .current_dir(fixture.root())
        .arg("review")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("olympe knows these projects"))
        .stderr(predicate::str::contains(format!("  project: {}", fixture.project().display())));
    assert_eq!(fixture.claude_log(), "");
}

#[test]
fn a_project_is_found_from_a_folder_inside_it_and_the_nearest_wins() {
    let fixture = Fixture::with_review();
    let inner = fixture.project().join("services/billing");
    std::fs::create_dir_all(inner.join("src")).unwrap();
    fixture
        .olympe()
        .current_dir(inner.join("src"))
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("review: Review a change"));

    fixture.olympe().current_dir(&inner).arg("init").assert().success();
    fixture
        .olympe()
        .current_dir(inner.join("src"))
        .arg("list")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no processes yet"));
    fixture.olympe().arg("list").assert().success();
}

// design

#[test]
fn design_opens_the_designer_in_the_folder_of_the_process() {
    let fixture = Fixture::new();
    let dir = fixture.home().join("review");
    fixture
        .olympe()
        .args(["design", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("created the process `review`: A process written by the fake designer."))
        .stdout(predicate::str::contains("reader  The reader of the fake team."))
        .stdout(predicate::str::contains(format!("the team is in {}", dir.display())))
        .stdout(predicate::str::contains("recorded as "))
        .stdout(predicate::str::contains("olympe review [task]"))
        .stdout(predicate::str::contains("to look at").not());

    for file in ["brief.md", "process.md", "specialists/reader.md", "specialists/tester.md"] {
        assert!(fixture.exists(&format!("review/{file}")), "{file}");
    }
    assert_eq!(fixture.project_files(), Vec::<String>::new());

    let calls = fixture.claude_calls();
    assert_eq!(calls.len(), 1);
    let (cwd, call) = &calls[0];
    // A session in the terminal, in the folder of the process, that reads the
    // project and its knowledge base and has no shell.
    assert_eq!(cwd, &dir);
    assert!(!call.starts_with("-p"));
    assert!(call.contains("--name project design review"));
    assert!(call.contains(&format!(
        "--add-dir {} {} --append-system-prompt You design teams of Claude agents for olympe.",
        fixture.project().display(),
        fixture.home().join("kb").display()
    )), "{call}");
    assert!(call.contains("## This session\n\nThe user is in this session"));
    assert!(call.contains("--restricted --tools Read,Write,Edit,Glob,Grep --permission-mode acceptEdits"));
    assert!(!call.contains("--permission-prompts"));
    assert!(!call.contains("--json-schema"));
    // What the designer is told to start from.
    let opening = &call[call.rfind("Design the `review` process and its team with me.").expect("the opening")..];
    assert!(opening.contains(&format!("The project `project` is in `{}`.", fixture.project().display())));
    assert!(opening.contains("It is empty: the process does not exist yet."));
    assert!(opening.contains("The project has no other process."));
}

#[test]
fn design_reopens_a_process_that_exists() {
    let fixture = Fixture::with_review();
    fixture.write("release/brief.md", "# release\n\n## Goal\nA version ships.\n");
    fixture.write("release/process.md", "---\ndescription: Ship a version.\n---\nThe writer writes the notes.\n");
    fixture.write("release/specialists/writer.md", "---\ndescription: Writes release notes.\n---\nYou write.\n");
    fixture.write("runs/review/20261127T101500Z/feedback.md", "## process: a");

    fixture
        .olympe()
        .args(["design", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("changed the process `review`: Review a change before it merges."));
    assert!(fixture.read("review/process.md").contains("REVISED."));

    let (cwd, call) = fixture.claude_calls().pop().unwrap();
    assert_eq!(cwd, fixture.home().join("review"));
    let runs_dir = fixture.home().join("runs/review");
    // The feedback that waits is for the designer to read.
    assert!(call.contains(&format!(
        "--add-dir {} {} {} --append-system-prompt",
        fixture.project().display(),
        fixture.home().join("kb").display(),
        runs_dir.display()
    )), "{call}");
    let opening = &call[call.rfind("Revise the `review` process and its team with me.").expect("the opening")..];
    assert!(opening.contains("Its specialists are in `specialists/`: reader, tester. It has no skill yet."));
    assert!(opening.contains(&format!(
        "The feedback of 1 run(s) of the process waits, one file for each run, oldest first:\n\
         - `{}/20261127T101500Z/feedback.md`\n",
        runs_dir.display()
    )), "{opening}");
    assert!(opening.contains("- `release`: Ship a version. Its specialists: writer."), "{opening}");
    // Reading the feedback does not use it.
    assert!(fixture.exists("runs/review/20261127T101500Z/feedback.md"));

    // A session that changes nothing leaves no trace.
    let before = fixture.history();
    fixture
        .olympe()
        .env("FAKE_REVISE", "nothing")
        .args(["design", "review"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("the process `review` is as it was"));
    assert_eq!(fixture.history(), before);
}

#[test]
fn a_design_session_that_writes_nothing_leaves_no_process() {
    let fixture = Fixture::new();
    fixture
        .olympe()
        .env("FAKE_DESIGN", "nothing")
        .args(["design", "review"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nothing was written: the project has no process `review`"));
    assert!(!fixture.exists("review"));
}

#[test]
fn a_design_session_can_be_resumed_on_what_olympe_cannot_load() {
    let fixture = Fixture::new();
    fixture
        .olympe()
        .env("FAKE_DESIGN", "broken")
        .args(["design", "review"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("the process `review` cannot run yet"))
        .stderr(predicate::str::contains("  - specialists/tester.md cannot be used"));
    // What was written is kept, and the user was there: nobody corrects it
    // behind their back.
    assert!(fixture.exists("review/process.md"));
    assert_eq!(fixture.claude_calls().len(), 1);
    assert_eq!(fixture.history()[0], "design review (cannot run yet)");

    fixture
        .olympe()
        .env("FAKE_REVISE", "repair")
        .args(["design", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("changed the process `review`"));
    let (_, call) = fixture.claude_calls().pop().unwrap();
    assert!(call.contains("Revise the `review` process"));
    assert!(call.contains(
        "It holds files that olympe cannot load as a process:\n- specialists/tester.md cannot be used"
    ), "{call}");
    assert_eq!(fixture.history()[0], "design review");
}

#[test]
fn design_auto_has_claude_write_the_team_alone() {
    let fixture = Fixture::new();
    fixture
        .olympe()
        .args(["design", "review", "--auto", "catch defects before merge"])
        .assert()
        .success()
        .stdout(predicate::str::contains("created the process `review`"))
        .stdout(predicate::str::contains("reader  The reader of the fake team."))
        .stdout(predicate::str::contains("A reader and a tester."))
        .stdout(predicate::str::contains(format!("cost $0.05; the team is in {}", fixture.home().join("review").display())))
        .stdout(predicate::str::contains("olympe review [task]"));

    assert!(fixture.exists("review/brief.md"));
    assert!(fixture.exists("review/specialists/tester.md"));
    assert_eq!(fixture.project_files(), Vec::<String>::new());

    let calls = fixture.claude_calls();
    assert_eq!(calls.len(), 1);
    let (cwd, call) = &calls[0];
    assert_eq!(cwd, &fixture.home().join("review"));
    assert!(call.starts_with("-p Design the `review` process and its team.\n"));
    assert!(call.contains("should achieve: catch defects before merge"));
    assert!(call.contains("Nobody answers in this session."));
    assert!(call.contains(&format!(
        "--add-dir {} {} --append-system-prompt",
        fixture.project().display(),
        fixture.home().join("kb").display()
    )));
    assert!(call.contains("--restricted --tools Read,Write,Edit,Glob,Grep"));
    assert!(call.contains("--json-schema"));
    assert!(call.contains("--permission-prompts none"));
    // The user is not there: the session's instructions are not given.
    assert!(!call.contains("## This session"));

    // On a process that exists, the request is a change.
    fixture
        .olympe()
        .args(["design", "review", "--auto", "add a security reviewer"])
        .assert()
        .success()
        .stdout(predicate::str::contains("changed the process `review`"))
        .stdout(predicate::str::contains("The playbook has a new phase."));
    let (_, call) = fixture.claude_calls().pop().unwrap();
    assert!(call.starts_with("-p Change the `review` process and its team.\n"));
    assert!(call.contains("What the user asks for: add a security reviewer"));
    assert!(fixture.read("review/process.md").contains("CHANGED."));
}

#[test]
fn design_refuses_a_name_it_cannot_use() {
    let fixture = Fixture::new();
    let refuse = |name: &str, reason: &str| {
        fixture
            .olympe()
            .args(["design", name])
            .assert()
            .code(2)
            .stderr(predicate::str::contains(reason));
    };
    refuse("list", "is an olympe command");
    refuse("design", "is an olympe command");
    refuse("check", "is an olympe command");
    refuse("kb", "knowledge base is kept");
    refuse("runs", "records of the runs are kept");
    refuse("Code Review", "not a usable name");
    assert_eq!(fixture.claude_log(), "");
}

#[test]
fn design_auto_lets_claude_correct_what_olympe_cannot_load() {
    let fixture = Fixture::new();
    fixture
        .olympe()
        .env("FAKE_DESIGN", "repair")
        .args(["design", "review", "--auto", "goal"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cost $0.10"));
    let calls = fixture.claude_calls();
    assert_eq!(calls.len(), 2);
    let (cwd, call) = &calls[1];
    assert_eq!(cwd, &fixture.home().join("review"));
    assert!(call.contains("olympe could not load what you wrote:\n- specialists/tester.md cannot be used"));
    assert!(call.contains("--resume fake-design"));
}

#[test]
fn design_auto_lets_claude_correct_what_the_check_finds() {
    let fixture = Fixture::new();
    fixture
        .olympe()
        .env("FAKE_DESIGN", "unused-skill")
        .args(["design", "review", "--auto", "goal"])
        .assert()
        .success()
        // What is still there after Claude's second go is for the user.
        .stdout(predicate::str::contains("to look at (`olympe design review` opens the designer on these):"))
        .stdout(predicate::str::contains("  - nothing calls for the skill `review:checklist`"));
    let calls = fixture.claude_calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].1.contains(
        "olympe checked the team and found:\n- nothing calls for the skill `review:checklist`"
    ), "{}", calls[1].1);
    assert!(calls[1].1.contains("--resume fake-design"));
}

#[test]
fn design_auto_fails_when_the_process_stays_unusable() {
    let fixture = Fixture::new();
    fixture
        .olympe()
        .env("FAKE_DESIGN", "broken")
        .args(["design", "review", "--auto", "goal"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("olympe cannot load"))
        .stderr(predicate::str::contains("`olympe design review` opens the designer"))
        .stderr(predicate::str::contains(fixture.home().join("review").display().to_string()))
        .stderr(predicate::str::contains("  - specialists/tester.md cannot be used"));
    assert_eq!(fixture.claude_calls().len(), 2);
    // What Claude wrote is kept, to correct.
    assert!(fixture.exists("review/process.md"));

    // When Claude wrote nothing, nothing is kept.
    fixture
        .olympe()
        .env("FAKE_DESIGN", "nothing")
        .args(["design", "release", "--auto", "goal"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("  - process.md does not exist"))
        .stderr(predicate::str::contains("  - brief.md does not exist"))
        .stderr(predicate::str::contains("  - specialists/ holds no specialist"));
    assert!(!fixture.exists("release"));
}

#[test]
fn design_reports_files_changed_in_the_project() {
    let fixture = Fixture::new();
    fixture.git(&["init", "-q"]);
    std::fs::write(fixture.project().join("dirty-before.txt"), "not Claude's doing").unwrap();
    for args in [&["design", "review"][..], &["design", "release", "--auto", "goal"]] {
        fixture
            .olympe()
            .env("FAKE_DESIGN", "stray")
            .args(args)
            .assert()
            .success()
            .stderr(predicate::str::contains("told to leave alone"))
            .stderr(predicate::str::contains(format!("  {}", fixture.project().join("stray.txt").display())))
            .stderr(predicate::str::contains("dirty-before.txt").not());
        std::fs::remove_file(fixture.project().join("stray.txt")).unwrap();
    }
}

// history

#[test]
fn every_change_of_a_team_is_a_commit_in_the_olympe_folder() {
    let fixture = Fixture::new();
    assert_eq!(fixture.history(), ["init project"]);
    // The knowledge base and the records of the runs have no place in it.
    assert_eq!(fixture.read(".gitignore"), "/kb/\n/runs/\n");

    fixture.olympe().args(["design", "review", "--auto", "goal"]).assert().success();
    assert_eq!(fixture.history(), ["design review", "init project"]);
    // What Claude said it did is the body of the commit.
    assert!(fixture.home_git(&["log", "-1", "--format=%b"]).contains("A reader and a tester."));

    // What the user changed by hand is not mistaken for Claude's work.
    fixture.write("review/specialists/reader.md", READER);
    fixture.write("runs/review/20261127T101500Z/feedback.md", "## process: a");
    fixture.olympe().args(["improve", "review"]).assert().success();
    assert_eq!(
        fixture.history(),
        [
            "improve review: the feedback of 1 run(s)",
            "changes made outside olympe",
            "design review",
            "init project"
        ]
    );
    let improvement = fixture.home_git(&["show", "--stat", "--format=%b"]);
    assert!(improvement.contains("The playbook now names the test command."), "{improvement}");
    assert!(improvement.contains("review/process.md"), "{improvement}");
    assert!(!improvement.contains("reader.md"), "{improvement}");
    assert!(!improvement.contains("feedback"), "{improvement}");
    assert_eq!(fixture.home_git(&["status", "--porcelain"]), "");
}

#[test]
fn a_project_without_a_history_gets_one() {
    let fixture = Fixture::with_review();
    std::fs::remove_dir_all(fixture.home().join(".git")).unwrap();
    fixture.olympe().args(["design", "review"]).assert().success();
    assert_eq!(fixture.history(), ["design review", "what olympe kept before it kept a history"]);
}

// check

#[test]
fn check_says_whether_a_team_can_run_and_what_to_look_at() {
    let fixture = Fixture::new();
    fixture
        .olympe()
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no processes yet"));

    let fixture = Fixture::with_review();
    fixture
        .olympe()
        .args(["check", "review"])
        .assert()
        .success()
        .stdout("review: can run, and nothing to look at\n");

    // A specialist no step uses, a skill nobody is told to use, and a skill
    // that is called for and does not exist.
    fixture.write("review/specialists/security.md", "---\ndescription: Reads for holes.\n---\nUse `review:threat-model`.\n");
    fixture.write("review/skills/checklist/SKILL.md", "---\nname: checklist\ndescription: The checks.\n---\nRun the suite.\n");
    fixture
        .olympe()
        .args(["check", "review"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("review: can run; to look at:"))
        .stdout(predicate::str::contains("  - the playbook never names the specialist `security`"))
        .stdout(predicate::str::contains(
            "  - specialists/security.md calls for the skill `review:threat-model`, which this team does not have",
        ))
        .stdout(predicate::str::contains("  - nothing calls for the skill `review:checklist`"))
        .stdout(predicate::str::contains("reader`").not())
        .stderr(predicate::str::contains("olympe design <process>"));

    // Every process, and one that cannot run is a failure.
    fixture.write("broken/process.md", "nothing");
    fixture
        .olympe()
        .arg("check")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("broken: the process `broken` in"))
        .stdout(predicate::str::contains("  - brief.md does not exist"))
        .stdout(predicate::str::contains("review: can run; to look at:"));
    fixture
        .olympe()
        .args(["check", "deploy"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("has no process named `deploy`"));
    assert_eq!(fixture.claude_log(), "");
}

#[test]
fn check_finds_the_name_of_a_specialist_that_left_the_team() {
    let fixture = Fixture::with_review();
    fixture.write(
        "review/process.md",
        "---\ndescription: Review a change.\n---\nThe `reader` reads the diff, then hands it to `tester`.\n",
    );
    fixture.olympe().args(["check", "review"]).assert().success();

    let finding = "process.md names `tester`, which is no longer a specialist of this team";
    fixture
        .olympe()
        .env("FAKE_REVISE", "drop-tester")
        .args(["design", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("to look at (`olympe design review` opens the designer on these):"))
        .stdout(predicate::str::contains(format!("  - {finding}")));
    fixture
        .olympe()
        .args(["check", "review"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(format!("  - {finding}")));

    // The designer is told what olympe found when it is opened again.
    fixture.olympe().env("FAKE_REVISE", "nothing").args(["design", "review"]).assert().code(1);
    let (_, call) = fixture.claude_calls().pop().unwrap();
    assert!(call.contains(&format!("olympe checked the team and found:\n- {finding}")), "{call}");
}

// run

#[test]
fn a_process_runs_in_the_project_folder_as_a_lead_session_with_its_team() {
    let fixture = Fixture::with_review();
    std::fs::create_dir(fixture.project().join("src")).unwrap();
    fixture
        .olympe()
        .current_dir(fixture.project().join("src"))
        .args(["review", "the", "login", "change", "--no-retro"])
        .assert()
        .success();

    let calls = fixture.claude_calls();
    assert_eq!(calls.len(), 1, "no retrospective with --no-retro");
    let (cwd, call) = &calls[0];
    assert_eq!(cwd, &fixture.project());
    assert!(!call.starts_with("-p"));
    assert!(call.contains("--name project review"));
    // The specialists, as agents the lead can start.
    assert!(call.contains(r#""reader":{"description":"Reads a diff for defects.","disallowedTools":["Edit","Write"],"prompt":"You read diffs.\n"#), "{call}");
    assert!(call.contains(r#""tester":{"description":"Runs the test suite.","model":"sonnet","prompt":"#));
    assert!(call.contains("You are the `tester` specialist of the `review` team of project."));
    // The lead's instructions.
    assert!(call.contains("# You lead the `review` team of project"));
    assert!(call.contains("- `reader`: Reads a diff for defects."));
    assert!(call.contains("- `tester`: Runs the test suite.\n\n## The brief"), "{call}");
    // What the user decided, then how the team does it.
    assert!(call.contains("## Goal\nNo defect reaches the main branch.\n\n## The playbook"), "{call}");
    assert!(call.contains("The reader reads the diff, then the tester runs the suite."));
    let kb = fixture.home().join("kb");
    assert!(call.contains(&format!("akashic search <words> --kb '{}'", kb.display())));
    // The task.
    assert!(call.trim_end().ends_with("Run the `review` process.\n\nThe task: the login change"));
    for placeholder in ["{process}", "{project}", "{name}", "{root}", "{roster}", "{skills}", "{brief}", "{playbook}", "{kb}"] {
        assert!(!call.contains(placeholder), "{placeholder} was left unfilled:\n{call}");
    }
    // A team with no skill loads none, and nothing is written without a
    // retrospective.
    assert!(!call.contains("--plugin-dir"));
    assert!(!call.contains("--add-dir"));
    assert!(!fixture.exists("runs"));
    assert_eq!(fixture.project_files(), ["src"]);
}

#[test]
fn a_specialist_belongs_to_one_process() {
    let fixture = Fixture::with_review();
    fixture.write("release/brief.md", "# release\n\n## Goal\nA version ships.\n");
    fixture.write("release/process.md", "---\ndescription: Ship a version.\n---\nThe reader reads the changelog.\n");
    fixture.write(
        "release/specialists/reader.md",
        "---\ndescription: Reads the changelog.\n---\nYou read changelogs.\n",
    );

    fixture.olympe().args(["release", "--no-retro"]).assert().success();
    let (_, call) = fixture.claude_calls().pop().unwrap();
    assert!(call.contains(r#""reader":{"description":"Reads the changelog.","prompt":"You read changelogs.\n"#), "{call}");
    assert!(!call.contains("You read diffs."));
    assert!(!call.contains("tester"));

    fixture.olympe().args(["review", "--no-retro"]).assert().success();
    let (_, call) = fixture.claude_calls().pop().unwrap();
    assert!(call.contains("You read diffs."));
    assert!(!call.contains("You read changelogs."));
}

#[test]
fn a_session_gets_the_skills_of_its_process() {
    let fixture = Fixture::new();
    fixture
        .olympe()
        .env("FAKE_DESIGN", "skill")
        .args(["design", "review", "--auto", "goal"])
        .assert()
        .success()
        .stdout(predicate::str::contains("  skills: review:checklist"));
    assert!(fixture.exists("review/skills/checklist/SKILL.md"));

    fixture.olympe().args(["review", "--no-retro"]).assert().success();
    let (_, call) = fixture.claude_calls().pop().unwrap();
    // Claude Code loads the folder of the process as a plugin: its skills.
    assert!(call.contains(&format!("--plugin-dir {} --agents", fixture.home().join("review").display())), "{call}");
    assert!(call.contains(
        "## The team's skills\n\nProcedures of this project"
    ), "{call}");
    assert!(call.contains(
        "- `review:checklist`: The checks a change must pass. Use before approving a change.\n\n## The brief"
    ), "{call}");

    fixture
        .olympe()
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("  skills: review:checklist"));
}

#[test]
fn run_is_the_long_form() {
    let fixture = Fixture::with_review();
    fixture.olympe().args(["run", "review", "--no-retro"]).assert().success();
    let calls = fixture.claude_calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].1.contains("I gave no task"));
}

#[test]
fn a_run_ends_with_a_retrospective() {
    let fixture = Fixture::with_review();
    let runs_dir = fixture.home().join("runs/review");
    fixture
        .olympe()
        .arg("review")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("retrospective written in {}/", runs_dir.display())))
        .stdout(predicate::str::contains("the feedback of 1 run(s) waits; `olympe improve review`"));

    let runs = fixture.runs("review");
    assert_eq!(runs.len(), 1);
    assert!(runs[0].starts_with("20") && runs[0].ends_with('Z'), "{runs:?}");
    let run_dir = runs_dir.join(&runs[0]);

    let calls = fixture.claude_calls();
    assert_eq!(calls.len(), 2);
    let (lead_cwd, lead) = &calls[0];
    let (retro_cwd, retro) = &calls[1];
    // The session can write in the folder of its run, which is outside the
    // project.
    assert!(lead.contains(&format!("--add-dir {} --agents", run_dir.display())), "{lead}");
    // The retrospective continues the lead's session, in the same folder and
    // with the same team.
    let session = value_after(lead, "--session-id ");
    assert_eq!(session.len(), 36);
    assert_eq!(retro_cwd, lead_cwd);
    assert!(retro.starts_with("-p The session is over"));
    assert_eq!(value_after(retro, "--resume "), session);
    assert!(retro.contains(&format!("--add-dir {} --agents", run_dir.display())));
    assert!(retro.contains("# You lead the `review` team of project"));
    assert!(retro.contains("--permission-mode acceptEdits --permission-prompts none"));
    assert!(retro.contains(&format!("`{}/learnings.md`", run_dir.display())));

    // The learnings go to the knowledge base.
    let log = fixture.akashic_log();
    let ingest = log.lines().find(|line| line.starts_with("ingest ")).expect("an ingest");
    assert_eq!(
        ingest.split(" --title review run 20").next().unwrap(),
        format!("ingest {}", run_dir.join("learnings.md").display())
    );
    assert!(ingest.ends_with(&format!("--kb {}", fixture.home().join("kb").display())), "{ingest}");
    assert_eq!(fixture.project_files(), Vec::<String>::new());
}

#[test]
fn a_run_with_nothing_learned_records_nothing() {
    let fixture = Fixture::with_review();
    fixture
        .olympe()
        .env("FAKE_RETRO", "none")
        .arg("review")
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to record"));
    assert!(!fixture.akashic_log().contains("ingest"));
    assert_eq!(fixture.runs("review"), Vec::<String>::new());

    fixture
        .olympe()
        .env("FAKE_RETRO", "feedback")
        .arg("review")
        .assert()
        .success()
        .stdout(predicate::str::contains("the feedback of 1 run(s) waits"));
    assert!(!fixture.akashic_log().contains("ingest"));
    assert_eq!(fixture.runs("review").len(), 1);
}

#[test]
fn a_failed_retrospective_is_a_warning() {
    let fixture = Fixture::with_review();
    fixture
        .olympe()
        .env("FAKE_RETRO", "fail")
        .arg("review")
        .assert()
        .success()
        .stderr(predicate::str::contains("no retrospective for this run"))
        .stderr(predicate::str::contains("No conversation found"))
        .stderr(predicate::str::contains("claude --resume "));
    assert_eq!(fixture.runs("review"), Vec::<String>::new());
}

#[test]
fn a_failed_ingest_keeps_the_learnings_and_says_how_to_record_them() {
    let fixture = Fixture::with_review();
    fixture
        .olympe()
        .env("FAKE_AKASHIC", "fail-ingest")
        .env("FAKE_RETRO", "learnings")
        .arg("review")
        .assert()
        .success()
        .stderr(predicate::str::contains("not in the knowledge base yet"))
        .stderr(predicate::str::contains("Record them with: akashic ingest '"));
    let runs = fixture.runs("review");
    assert!(fixture.exists(&format!("runs/review/{}/learnings.md", runs[0])));
}

#[test]
fn a_process_that_cannot_run_starts_no_session() {
    let fixture = Fixture::with_review();
    fixture
        .olympe()
        .arg("deploy")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("the project `project` has no process named `deploy`"))
        .stderr(predicate::str::contains("olympe design deploy"));

    fixture.write("review/specialists/tester.md", "---\ndescription: Tests.\ncolor: red\n---\nYou test.\n");
    fixture.write("review/skills/checklist/notes.md", "no SKILL.md here");
    std::fs::remove_file(fixture.home().join("review/brief.md")).unwrap();
    fixture
        .olympe()
        .arg("review")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("  - brief.md does not exist"))
        .stderr(predicate::str::contains(format!(
            "the process `review` in {} cannot run",
            fixture.home().join("review").display()
        )))
        .stderr(predicate::str::contains("  - specialists/tester.md cannot be used"))
        .stderr(predicate::str::contains("unknown field `color`"))
        .stderr(predicate::str::contains("  - skills/checklist/SKILL.md does not exist"));

    std::fs::remove_dir_all(fixture.home().join("review/skills")).unwrap();
    std::fs::remove_dir_all(fixture.home().join("review/specialists")).unwrap();
    fixture
        .olympe()
        .arg("review")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("specialists/ holds no specialist"));
    assert_eq!(fixture.claude_log(), "");
}

// list

#[test]
fn list_shows_the_processes_and_the_feedback_that_waits() {
    let fixture = Fixture::new();
    fixture
        .olympe()
        .arg("list")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no processes yet"));

    let fixture = Fixture::with_review();
    fixture.write("broken/process.md", "nothing");
    fixture.olympe().arg("review").assert().success();
    fixture
        .olympe()
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("review: Review a change before it merges."))
        .stdout(predicate::str::contains("  team: reader, tester"))
        .stdout(predicate::str::contains("the feedback of 1 run(s) waits: olympe improve review"))
        .stdout(predicate::str::contains("broken: the process `broken` in"))
        .stdout(predicate::str::contains("  - process.md cannot be used"))
        // The knowledge base is no process.
        .stdout(predicate::str::contains("kb:").not());
}

// improve

#[test]
fn improve_changes_the_team_from_the_feedback_of_its_runs() {
    let fixture = Fixture::with_review();
    fixture
        .olympe()
        .args(["improve", "review"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no feedback waits for `review`"));
    assert_eq!(fixture.claude_log(), "");

    fixture.olympe().arg("review").assert().success();
    let run = fixture.runs("review").remove(0);
    fixture
        .olympe()
        .args(["improve", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("improved `review` from the feedback of 1 run(s)"))
        .stdout(predicate::str::contains("The playbook now names the test command."))
        .stdout(predicate::str::contains(format!("the team is in {}", fixture.home().join("review").display())));

    // The improvement happens in the folder of the process.
    let (cwd, call) = fixture.claude_calls().pop().unwrap();
    assert_eq!(cwd, fixture.home().join("review"));
    assert!(call.contains("Improve the `review` process"));
    assert!(call.contains(
        "Its brief is `brief.md`, and the process is `process.md`. Its specialists are in \
         `specialists/`: reader, tester. It has no skill yet."
    ));
    assert!(call.contains("you do not change `brief.md`"));
    let runs_dir = fixture.home().join("runs/review");
    assert!(call.contains(&format!("- `{}/{run}/feedback.md`", runs_dir.display())));
    assert!(call.contains(&format!(
        "--add-dir {} {} {} --append-system-prompt",
        fixture.project().display(),
        fixture.home().join("kb").display(),
        runs_dir.display()
    )));
    assert!(call.contains("--restricted"));
    assert!(call.contains("--json-schema"));
    assert!(fixture.read("review/process.md").contains("IMPROVED."));
    assert_eq!(fixture.project_files(), Vec::<String>::new());

    // The feedback is kept, and no longer waits.
    assert!(!fixture.exists(&format!("runs/review/{run}/feedback.md")));
    assert!(fixture.exists(&format!("runs/review/{run}/feedback.applied.md")));
    fixture.olympe().args(["improve", "review"]).assert().code(1);
}

#[test]
fn improve_reads_only_the_feedback_of_its_process() {
    let fixture = Fixture::with_review();
    fixture.write("review/skills/checklist/SKILL.md", "---\nname: checklist\ndescription: The checks.\n---\nRun the suite.\n");
    fixture.write("review/specialists/tester.md", &format!("{TESTER}Use `review:checklist` first.\n"));
    fixture.write("runs/review/20261127T101500Z/feedback.md", "## process: a");
    fixture.write("runs/review/20261126T101500Z/feedback.md", "## process: b");
    fixture.write("runs/release/20261127T101500Z/feedback.md", "## process: c");
    fixture.write("runs/review/20261125T101500Z/feedback.applied.md", "## process: d");
    fixture.write("runs/review/20261124T101500Z/learnings.md", "## e");

    fixture
        .olympe()
        .env("FAKE_IMPROVE", "unchanged")
        .args(["improve", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("left `review` as it is after reading the feedback of 2 run(s)"));
    let (_, call) = fixture.claude_calls().pop().unwrap();
    let runs_dir = fixture.home().join("runs/review");
    assert!(call.contains(&format!(
        "oldest first:\n- `{0}/20261126T101500Z/feedback.md`\n- `{0}/20261127T101500Z/feedback.md`\n\n",
        runs_dir.display()
    )), "{call}");
    assert!(call.contains("Its skills are in `skills/`: checklist."));
    assert!(fixture.exists("runs/release/20261127T101500Z/feedback.md"));
    assert!(fixture.exists("runs/review/20261127T101500Z/feedback.applied.md"));
}

#[test]
fn improve_keeps_the_feedback_when_the_process_ends_up_unusable() {
    let fixture = Fixture::with_review();
    fixture.write("runs/review/20261127T101500Z/feedback.md", "## process: a");
    fixture
        .olympe()
        .env("FAKE_IMPROVE", "breaks")
        .args(["improve", "review"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("olympe cannot load"));
    assert!(fixture.exists("runs/review/20261127T101500Z/feedback.md"));
}

#[test]
fn improve_does_not_change_the_brief() {
    let fixture = Fixture::with_review();
    fixture.write("runs/review/20261127T101500Z/feedback.md", "## process: a");
    fixture
        .olympe()
        .env("FAKE_IMPROVE", "brief")
        .args(["improve", "review"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Claude changed brief.md, which only you change; olympe put it back"))
        .stdout(predicate::str::contains("improved `review`"));
    assert_eq!(fixture.read("review/brief.md"), BRIEF);
    // The rest of the improvement stands.
    assert!(fixture.read("review/process.md").contains("IMPROVED."));
    assert!(!fixture.home_git(&["show", "--stat", "--format="]).contains("brief.md"));
}
