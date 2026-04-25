//! Entry point for the `talon` binary.

use std::process::ExitCode;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    if let Err(error) = color_eyre::install() {
        eprintln!("failed to install color-eyre error reporting: {error}");
    }

    let code = talon_cli::run().await;
    ExitCode::from(code)
}
