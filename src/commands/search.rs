use clap::Args;

#[derive(Args)]
pub struct SearchArgs {
    /// Natural language query
    pub query: String,
    /// Maximum results
    #[arg(long, default_value = "10")]
    pub limit: usize,
}

pub fn run(args: SearchArgs) {
    println!("search \"{}\" (limit: {})", args.query, args.limit);
}
