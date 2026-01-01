use clap::Subcommand;

#[derive(Subcommand)]
pub enum QueryCommand {
    /// Get function details
    Function {
        /// Qualified function name
        qualified_name: String,
    },

    /// Trace call graph
    Trace {
        /// Qualified function name
        qualified_name: String,
        /// Trace depth
        #[arg(long, default_value = "2")]
        depth: usize,
        /// Token budget for output
        #[arg(long)]
        budget: Option<usize>,
    },

    /// Find all usages of a symbol
    Usages {
        /// Symbol name
        symbol: String,
        /// Token budget for output
        #[arg(long)]
        budget: Option<usize>,
    },

    /// Get file overview
    File {
        /// File path
        path: String,
    },

    /// List all functions in a file
    List {
        /// File path
        path: String,
    },
}

pub fn run(cmd: QueryCommand) {
    match cmd {
        QueryCommand::Function { qualified_name } => {
            println!("query function {}", qualified_name);
        }
        QueryCommand::Trace {
            qualified_name,
            depth,
            budget,
        } => {
            println!(
                "query trace {} (depth: {}, budget: {:?})",
                qualified_name, depth, budget
            );
        }
        QueryCommand::Usages { symbol, budget } => {
            println!("query usages {} (budget: {:?})", symbol, budget);
        }
        QueryCommand::File { path } => {
            println!("query file {}", path);
        }
        QueryCommand::List { path } => {
            println!("query list {}", path);
        }
    }
}
