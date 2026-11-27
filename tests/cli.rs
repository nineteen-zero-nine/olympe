use assert_cmd::Command;
use predicates::prelude::*;

fn olympe() -> Command {
    Command::cargo_bin("olympe").expect("binary builds")
}

#[test]
fn hello_defaults_to_world() {
    olympe()
        .arg("hello")
        .assert()
        .success()
        .stdout("Hello, world!\n");
}

#[test]
fn hello_greets_given_name() {
    olympe()
        .args(["hello", "Zeus"])
        .assert()
        .success()
        .stdout("Hello, Zeus!\n");
}

#[test]
fn no_subcommand_prints_usage_and_fails() {
    olympe()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
}
