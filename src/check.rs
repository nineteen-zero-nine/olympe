//! What olympe can tell about a team without running it: whether it loads,
//! and where its files no longer agree with each other.

use std::collections::BTreeSet;

use crate::history;
use crate::project::Project;
use crate::team::{PROCESS_FILE, SKILL_FILE, SKILLS_DIR, SPECIALISTS_DIR, Team};

fn is_name_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
}

/// Whether `text` holds `name` as a word of its own, in any case.
fn mentions(text: &str, name: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        !before.is_some_and(is_name_char) && !after.is_some_and(is_name_char)
    })
}

/// The skills `text` refers to by their full name, `<process>:<skill>`, as
/// the names of the skills.
fn skill_references(text: &str, process: &str) -> BTreeSet<String> {
    let prefix = format!("{process}:");
    text.match_indices(&prefix)
        .filter(|(at, _)| !text[..*at].chars().next_back().is_some_and(is_name_char))
        .map(|(at, _)| {
            text[at + prefix.len()..]
                .chars()
                .take_while(|c| is_name_char(*c))
                .collect::<String>()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// Where the files of a team that loads no longer agree with each other, one
/// line each. These do not keep the team from running.
pub fn findings(project: &Project, team: &Team) -> Vec<String> {
    let process = &team.process;
    // Every text of the team that someone follows, with the file it is in.
    let mut texts = vec![(PROCESS_FILE.to_string(), process.playbook.as_str())];
    for agent in &team.specialists {
        texts.push((format!("{SPECIALISTS_DIR}/{}.md", agent.name), agent.prompt.as_str()));
    }
    for skill in &team.skills {
        texts.push((format!("{SKILLS_DIR}/{}/{SKILL_FILE}", skill.name), skill.procedure.as_str()));
    }
    let mut findings = Vec::new();

    for agent in &team.specialists {
        if !mentions(&process.playbook, &agent.name) {
            findings.push(format!(
                "the playbook never names the specialist `{}`: the lead has no step to give it",
                agent.name
            ));
        }
    }

    // A name in backticks is a name on purpose; the same word in a sentence
    // may be a word.
    for former in history::former_specialists(&project.home, &process.name, &team.specialist_names()) {
        let quoted = format!("`{former}`");
        for (file, text) in &texts {
            if text.contains(&quoted) {
                findings.push(format!(
                    "{file} names `{former}`, which is no longer a specialist of this team"
                ));
            }
        }
    }

    let mut used = BTreeSet::new();
    for (file, text) in &texts {
        for name in skill_references(text, &process.name) {
            if !team.skills.iter().any(|skill| skill.name == name) {
                findings.push(format!(
                    "{file} calls for the skill `{}:{name}`, which this team does not have",
                    process.name
                ));
            }
            // A skill that names itself is not called for by anyone.
            if *file != format!("{SKILLS_DIR}/{name}/{SKILL_FILE}") {
                used.insert(name);
            }
        }
    }
    for skill in &team.skills {
        if !used.contains(&skill.name) {
            findings.push(format!(
                "nothing calls for the skill `{}:{}`: neither the playbook nor a specialist's \
                 instructions say when to use it",
                process.name, skill.name
            ));
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_mentioned_as_a_word_of_its_own() {
        assert!(mentions("The Reader reads the diff.", "reader"));
        assert!(mentions("Hand it to `reader`.", "reader"));
        assert!(!mentions("The proof-reader reads.", "reader"));
        assert!(!mentions("The readers read.", "reader"));
        assert!(!mentions("", "reader"));
    }

    #[test]
    fn finds_the_skills_a_text_calls_for() {
        let found = skill_references("Use `review:checklist`, then review:tone-check. See pre-review:x.", "review");
        assert_eq!(found.into_iter().collect::<Vec<_>>(), ["checklist", "tone-check"]);
        assert!(skill_references("The review: read it.", "review").is_empty());
    }
}
