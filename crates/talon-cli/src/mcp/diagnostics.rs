use std::backtrace::Backtrace;
use std::fmt::Write as _;
use std::io::Write as _;
use std::panic::PanicHookInfo;
use std::path::PathBuf;
use std::process;
use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};

use fs_err as fs;

static INSTALL_PANIC_HOOK: Once = Once::new();

const READY_FILE: &str = "mcp-ready";
const CRASH_FILE: &str = "mcp-last-crash";

#[derive(Debug)]
pub struct McpReadyGuard;

impl Drop for McpReadyGuard {
    fn drop(&mut self) {
        clear_ready();
    }
}

#[must_use]
pub const fn ready_guard() -> McpReadyGuard {
    McpReadyGuard
}

pub fn install_panic_hook() {
    INSTALL_PANIC_HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            record_panic("panic hook", info);
            clear_ready();
        }));
    });
}

pub fn mark_ready() {
    let _ = write_marker(
        READY_FILE,
        &format!("pid={}\nstarted_ms={}\n", process::id(), now_ms()),
    );
}

pub fn clear_ready() {
    let _ = fs::remove_file(state_file(READY_FILE));
}

pub fn record_caught_panic(context: &str, payload: &(dyn std::any::Any + Send)) {
    let mut message = String::new();
    let _ = writeln!(message, "context={context}");
    let _ = writeln!(message, "pid={}", process::id());
    let _ = writeln!(message, "timestamp_ms={}", now_ms());
    let _ = writeln!(message, "payload={}", panic_payload(payload));
    let _ = writeln!(message, "backtrace={}", Backtrace::force_capture());
    let _ = write_crash_report(&message);
}

#[must_use]
pub fn crash_status_warning() -> Option<String> {
    let path = state_file(CRASH_FILE);
    let report = fs::read_to_string(&path).ok()?;
    let first_lines = report.lines().take(3).collect::<Vec<_>>().join("; ");
    Some(format!(
        "last MCP panic recorded at {}: {first_lines}",
        path.display()
    ))
}

fn record_panic(context: &str, info: &PanicHookInfo<'_>) {
    let mut message = String::new();
    let _ = writeln!(message, "context={context}");
    let _ = writeln!(message, "pid={}", process::id());
    let _ = writeln!(message, "timestamp_ms={}", now_ms());
    let _ = writeln!(message, "panic={info}");
    let _ = writeln!(message, "backtrace={}", Backtrace::force_capture());
    let _ = write_crash_report(&message);
}

fn write_crash_report(report: &str) -> std::io::Result<()> {
    write_marker(CRASH_FILE, report)?;
    let path = state_file(&format!(
        "talon-mcp-panic-{}-{}.log",
        now_ms(),
        process::id()
    ));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    file.write_all(report.as_bytes())
}

fn write_marker(file_name: &str, contents: &str) -> std::io::Result<()> {
    let path = state_file(file_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

fn state_file(file_name: &str) -> PathBuf {
    state_dir().join(file_name)
}

fn state_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("talon")
}

fn panic_payload(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "<non-string panic payload>"
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}
