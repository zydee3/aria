use clap::Subcommand;

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Set a configuration value
    Set {
        /// Configuration key
        key: String,
        /// Configuration value
        value: String,
    },
}

pub fn run(cmd: ConfigCommand) {
    match cmd {
        ConfigCommand::Set { key, value } => {
            println!("config set {} = {}", key, value);
        }
    }
}
