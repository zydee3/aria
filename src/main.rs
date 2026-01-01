mod commands;

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
    /// Initialize aria in a repository
    Init(commands::init::InitArgs),

    /// Build full index
    Index,

    /// Incremental index update
    Update(commands::update::UpdateArgs),

    /// Check if index is current
    Check,

    /// Validate index integrity
    Validate,

    /// Show index statistics
    Stats,

    /// Query the index
    Query {
        #[command(subcommand)]
        cmd: commands::query::QueryCommand,
    },

    /// Semantic search over embeddings
    Search(commands::search::SearchArgs),

    /// Manage configuration
    Config {
        #[command(subcommand)]
        cmd: commands::config::ConfigCommand,
    },

    /// Manage git hooks
    Hooks {
        #[command(subcommand)]
        cmd: commands::hooks::HooksCommand,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Init(args) => commands::init::run(args),
        Command::Index => commands::index::run(),
        Command::Update(args) => commands::update::run(args),
        Command::Check => commands::check::run(),
        Command::Validate => commands::validate::run(),
        Command::Stats => commands::stats::run(),
        Command::Query { cmd } => commands::query::run(cmd),
        Command::Search(args) => commands::search::run(args),
        Command::Config { cmd } => commands::config::run(cmd),
        Command::Hooks { cmd } => commands::hooks::run(cmd),
    }
}
