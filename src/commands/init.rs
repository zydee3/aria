use clap::Args;

#[derive(Args)]
pub struct InitArgs {
    /// Exclude .aria/ from git
    #[arg(long)]
    pub local: bool,
}

pub fn run(args: InitArgs) {
    println!("init (local: {})", args.local);
}
