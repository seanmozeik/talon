use crate::cli::InitArgs;
use crate::config;
use eyre::Result;
use std::io::{self, Write as _};

pub(super) fn emit(_args: &InitArgs) -> Result<()> {
    let result = config::init_config()?;
    crate::banner::clear_fancy_prelude();
    let mut stderr = io::stderr().lock();
    if result {
        writeln!(
            stderr,
            "Created {}",
            config::default_config_path().display()
        )?;
    } else {
        writeln!(stderr, "Exists {}", config::default_config_path().display())?;
    }
    Ok(())
}
