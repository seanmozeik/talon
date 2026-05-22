use eyre::Result;
use std::io::{self, Write};
use talon_core::config::keychain;

#[derive(Debug, Clone, clap::Subcommand)]
pub enum SecretsSubcommand {
    Set { name: String, key: String },
    Delete { name: String },
    Status,
}

pub fn emit(sub: &SecretsSubcommand) -> Result<()> {
    let mut stdout = io::stdout().lock();
    match sub {
        SecretsSubcommand::Set { name, key } => {
            keychain::set(name, key)?;
            writeln!(stdout, "saved: {name}")?;
        }
        SecretsSubcommand::Delete { name } => {
            if keychain::delete(name)? {
                writeln!(stdout, "deleted: {name}")?;
            } else {
                writeln!(stdout, "not found: {name}")?;
            }
        }
        SecretsSubcommand::Status => {
            let names = keychain::list_names()?;
            if names.is_empty() {
                writeln!(stdout, "no credentials stored")?;
            } else {
                for name in names {
                    writeln!(stdout, "● {name}")?;
                }
            }
        }
    }
    Ok(())
}
