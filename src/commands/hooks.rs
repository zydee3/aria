use clap::Subcommand;

#[derive(Subcommand)]
pub enum HooksCommand {
    /// Install git hooks
    Install,
    /// Uninstall git hooks
    Uninstall,
}

pub fn run(cmd: HooksCommand) {
    match cmd {
        HooksCommand::Install => {
            println!("hooks install");
        }
        HooksCommand::Uninstall => {
            println!("hooks uninstall");
        }
    }
}
