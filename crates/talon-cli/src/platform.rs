//! Platform initialization and terminal detection.

/// Initializes platform-specific terminal behavior.
#[cfg(not(windows))]
pub const fn start() {}

/// Initializes platform-specific terminal behavior.
#[cfg(windows)]
pub fn start() {
    start_windows();
}

#[cfg(windows)]
fn start_windows() {
    use windows_sys::Win32::System::Console::SetConsoleOutputCP;
    // SAFETY: `SetConsoleOutputCP` takes a codepage id and does not dereference pointers.
    let result = unsafe { SetConsoleOutputCP(65_001) };
    if result == 0 {
        tracing::warn!("failed to configure Windows console output as UTF-8");
    }
}

/// Returns whether stdout is connected to an interactive terminal.
#[must_use]
pub fn stdout_is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// Returns whether stderr is connected to an interactive terminal.
#[must_use]
pub fn stderr_is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

/// Returns whether the user permits ANSI color.
#[must_use]
pub fn user_accepts_ansi_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::env::var("CLICOLOR").as_deref() != Ok("0")
}
