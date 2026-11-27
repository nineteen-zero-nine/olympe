use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "olympe", version, about, propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print a greeting
    Hello {
        /// Who to greet
        #[arg(default_value = "world")]
        name: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Hello { name } => hello(&name),
    }
}

fn hello(name: &str) -> Result<()> {
    println!("Hello, {name}!");
    Ok(())
}
