//! A project: a folder where teams work, and the folder olympe keeps for it,
//! `~/.olympe/<project>/`, which holds its teams and its knowledge base.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::akashic;
use crate::history;
use crate::text;

pub const CONFIG_FILE: &str = "olympe.toml";
/// Where `olympe init` creates the knowledge base, in the project's olympe
/// folder.
pub const KB_DIR: &str = "kb";

const CONFIG_TEMPLATE: &str = include_str!("../templates/olympe.toml");

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    /// The project's folder.
    pub root: String,
    /// The akashic knowledge base: a folder, or a name akashic knows.
    pub kb: String,
    pub claude: ClaudeConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClaudeConfig {
    pub bin: Option<String>,
    pub model: Option<String>,
    pub max_budget_usd: Option<f64>,
}

#[derive(Debug)]
pub struct Project {
    pub name: String,
    /// What olympe keeps for the project: `~/.olympe/<name>`.
    pub home: PathBuf,
    /// The project's folder, where its teams work.
    pub root: PathBuf,
    pub config: Config,
}

/// Where olympe keeps every project: `$OLYMPE_HOME`, else `~/.olympe`.
pub fn olympe_home() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("OLYMPE_HOME").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::home_dir().context("cannot find your home folder; set OLYMPE_HOME")?;
    Ok(home.join(".olympe"))
}

/// Every project olympe keeps, by name. One that cannot be read is skipped
/// with a warning, so that it does not stand in the way of the others.
fn all() -> Result<Vec<Project>> {
    let home = olympe_home()?;
    let entries = match std::fs::read_dir(&home) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("reading {}", home.display())),
    };
    let mut projects = Vec::new();
    for entry in entries {
        let dir = entry?.path();
        if !dir.join(CONFIG_FILE).is_file() {
            continue;
        }
        match Project::load(&dir) {
            Ok(project) => projects.push(project),
            Err(err) => eprintln!("warning: skipping the project in {}: {err:#}", dir.display()),
        }
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(projects)
}

fn current_dir() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("reading the current directory")?;
    cwd.canonicalize()
        .with_context(|| format!("resolving {}", cwd.display()))
}

impl Project {
    /// The project the current directory is in: the one whose folder is the
    /// current directory or the nearest of its parents.
    pub fn find() -> Result<Project> {
        let cwd = current_dir()?;
        let mut projects = all()?;
        let nearest = projects
            .iter()
            .enumerate()
            .filter(|(_, project)| cwd.starts_with(&project.root))
            .max_by_key(|(_, project)| project.root.components().count())
            .map(|(i, _)| i);
        if let Some(i) = nearest {
            return Ok(projects.swap_remove(i));
        }
        let mut message = format!(
            "no olympe project found for {}: run `olympe init` in the project's folder",
            cwd.display()
        );
        if !projects.is_empty() {
            message.push_str("\nolympe knows these projects:");
            for project in &projects {
                message.push_str(&format!("\n  {}: {}", project.name, project.root.display()));
            }
            message.push_str(&format!(
                "\nIf a project's folder moved, set `root` in its {CONFIG_FILE}, in {}/<project>/",
                olympe_home()?.display()
            ));
        }
        bail!(message)
    }

    /// The project kept in `home`, a folder of the olympe home.
    fn load(home: &Path) -> Result<Project> {
        let config_path = home.join(CONFIG_FILE);
        let text = std::fs::read_to_string(&config_path)
            .with_context(|| format!("reading {}", config_path.display()))?;
        let config: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", config_path.display()))?;
        let name = home
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| text::is_name(name))
            .with_context(|| {
                format!(
                    "the name of the folder {} must be made of lowercase letters, digits and hyphens",
                    home.display()
                )
            })?
            .to_string();
        let root = PathBuf::from(&config.root);
        if !root.is_absolute() {
            bail!(
                "{}: `root` must be the full path of the project's folder",
                config_path.display()
            );
        }
        // A folder that is gone keeps its path: it matches no directory.
        let root = root.canonicalize().unwrap_or(root);
        Ok(Project {
            name,
            home: home.to_path_buf(),
            root,
            config,
        })
    }

    /// The folder of the project's knowledge base.
    pub fn kb_dir(&self) -> Result<PathBuf> {
        resolve_kb(&self.home, &self.config.kb).with_context(|| {
            format!(
                "finding the knowledge base `{}` named in {}",
                self.config.kb,
                self.home.join(CONFIG_FILE).display()
            )
        })
    }
}

/// `given` as the folder of a knowledge base, absolute or relative to `base`.
fn kb_folder(base: &Path, given: &str) -> Option<PathBuf> {
    let dir = base.join(given);
    dir.join(akashic::CONFIG_FILE)
        .is_file()
        .then(|| dir.canonicalize().unwrap_or(dir))
}

/// A knowledge base given as a folder, absolute or relative to `base`, or as
/// a name akashic knows. A folder wins over a name.
fn resolve_kb(base: &Path, given: &str) -> Result<PathBuf> {
    if given.is_empty() {
        bail!("no knowledge base is set");
    }
    if let Some(dir) = kb_folder(base, given) {
        return Ok(dir);
    }
    if text::is_name(given)
        && let Some(dir) = akashic::named(given)?
    {
        if !dir.join(akashic::CONFIG_FILE).is_file() {
            bail!(
                "akashic knows `{given}` as {}, which is not a knowledge base",
                dir.display()
            );
        }
        return Ok(dir.canonicalize().unwrap_or(dir));
    }
    bail!(
        "`{given}` is neither a knowledge base folder nor a name that `akashic kb list` shows"
    )
}

/// Make the current folder a project: create its olympe folder, which will
/// hold its teams. With `kb`, the project takes that existing knowledge base;
/// without, a new one is created in the olympe folder.
pub fn init(name: Option<String>, kb: Option<String>) -> Result<()> {
    let root = current_dir()?;
    let name = match name {
        Some(name) => name,
        None => {
            let folder = root.file_name().map(|n| n.to_string_lossy().into_owned());
            let name = text::to_name(folder.as_deref().unwrap_or(""));
            if name.is_empty() {
                bail!("cannot make a project name from this folder's name; pass --name");
            }
            name
        }
    };
    if !text::is_name(&name) {
        bail!("`{name}` is not a usable name: use lowercase letters, digits and hyphens only");
    }

    let projects = all()?;
    if let Some(project) = projects.iter().find(|project| project.root == root) {
        bail!(
            "{} is already an olympe project: `{}`, kept in {}",
            root.display(),
            project.name,
            project.home.display()
        );
    }
    let home = olympe_home()?.join(&name);
    if home.exists() {
        match projects.iter().find(|project| project.name == name) {
            Some(project) => bail!(
                "the name `{name}` is taken by the project in {}; pass --name",
                project.root.display()
            ),
            None => bail!("{} already exists; pass --name, or remove it", home.display()),
        }
    }

    // The knowledge base first: it is the step that can fail.
    let kb = match kb {
        Some(given) => {
            // A folder is given from here, and kept as its full path.
            let (dir, kept) = match kb_folder(&root, &given) {
                Some(dir) => (dir.clone(), dir.display().to_string()),
                None => (resolve_kb(&root, &given)?, given),
            };
            // One knowledge base for one project.
            if let Some(owner) = projects
                .iter()
                .find(|project| project.kb_dir().is_ok_and(|theirs| theirs == dir))
            {
                bail!(
                    "{} is the knowledge base of the project `{}`; a project has its own",
                    dir.display(),
                    owner.name
                );
            }
            std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;
            println!("using the knowledge base at {}", dir.display());
            kept
        }
        None => {
            std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;
            if let Err(err) = akashic::init(&home.join(KB_DIR), &name) {
                // Nothing but this call's own leftovers can be in there.
                let _ = std::fs::remove_dir_all(&home);
                return Err(err);
            }
            KB_DIR.to_string()
        }
    };

    let quoted = |value: &str| toml::Value::String(value.to_string()).to_string();
    let config = text::fill(
        CONFIG_TEMPLATE,
        &[("root", &quoted(&root.display().to_string())), ("kb", &quoted(&kb))],
    );
    let config_path = home.join(CONFIG_FILE);
    std::fs::write(&config_path, config).with_context(|| format!("writing {}", config_path.display()))?;

    println!(
        "`{name}` is now an olympe project; its teams and its knowledge base are kept in {}",
        home.display()
    );
    history::record(&home, &format!("init {name}"));
    println!("next: `olympe design <process>` opens the designer, to make the project's first team");
    Ok(())
}
