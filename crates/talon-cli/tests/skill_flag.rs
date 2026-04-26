use std::process::Command;

use color_eyre::eyre::{Result, bail};

#[test]
fn skill_flag_prints_nonempty_content_and_exits_zero() -> Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_talon"))
        .arg("--skill")
        .output()?;

    if !output.status.success() {
        bail!(
            "talon --skill exited with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.is_empty() {
        bail!("talon --skill produced no output");
    }
    if !stdout.contains("talon") {
        bail!("SKILL.md output missing expected 'talon' content");
    }

    Ok(())
}
