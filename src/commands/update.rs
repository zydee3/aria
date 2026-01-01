use clap::Args;

#[derive(Args)]
pub struct UpdateArgs {
    /// Start commit
    #[arg(long)]
    pub from: Option<String>,
    /// End commit
    #[arg(long)]
    pub to: Option<String>,
    /// Only index staged files
    #[arg(long)]
    pub staged: bool,
}

pub fn run(args: UpdateArgs) {
    println!(
        "update (from: {:?}, to: {:?}, staged: {})",
        args.from, args.to, args.staged
    );
}
