mod commands;
mod config;
mod externals;
mod index;
mod parser;
mod resolver;
mod summarizer;
mod topo;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "aria")]
#[command(about = "Git-native codebase indexer for LLMs")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the index
    Index,

    /// Print raw source code for any symbol
    Source {
        /// Symbol name (exact, then contains match)
        name: String,
        /// Filter by kind: function, struct, enum, typedef, interface, variable
        #[arg(long, short = 'k')]
        kind: Option<String>,
    },

    /// Show call graph for a function
    Callstack {
        /// Function name (exact, then contains match)
        name: String,
        /// Show only forward trace (what this function calls)
        #[arg(long, short = 'f')]
        forward: bool,
        /// Show only backward trace (what calls this function)
        #[arg(long, short = 'b')]
        backward: bool,
        /// Depth limit (default: 2, 0 = unlimited)
        #[arg(long, short = 'd', default_value = "2")]
        depth: usize,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Index => commands::index::run(),
        Command::Source { name, kind } => commands::source::run(&name, kind.as_deref()),
        Command::Callstack { name, forward, backward, depth } => {
            commands::callstack::run(&name, forward, backward, depth)
        }
    }
}
