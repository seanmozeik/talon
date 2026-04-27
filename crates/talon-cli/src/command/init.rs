use crate::config;
use eyre::Result;

pub(super) fn emit() -> Result<()> {
    let result = config::init_config()?;
    if result {
        eprintln!("Created {}", config::default_config_path().display());
    } else {
        eprintln!("Exists {}", config::default_config_path().display());
    }
    Ok(())
}
