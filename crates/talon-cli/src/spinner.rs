//! Optional stderr spinner for human-oriented command runs.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rattles::presets::prelude as presets;
use tokio::time::MissedTickBehavior;

/// Runs `work` while showing a spinner on stderr.
///
/// # Errors
///
/// Propagates errors from `work`.
pub async fn with_spinner<F, T, E>(label: String, work: F) -> Result<T, E>
where
    F: std::future::Future<Output = Result<T, E>>,
{
    let done = Arc::new(AtomicBool::new(false));
    let done_spinner = Arc::clone(&done);

    let mut rattle = presets::pulse().into_ticked();
    let period = rattle.interval();

    let spinner = tokio::task::spawn(async move {
        let mut stderr = std::io::stderr();
        let mut tick = tokio::time::interval(period);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        while !done_spinner.load(Ordering::Acquire) {
            let _ = write!(&mut stderr, "\r\x1b[K{} {} ", rattle.current_frame(), label);
            let _ = stderr.flush();
            tick.tick().await;
            rattle.tick();
        }
        let _ = write!(&mut stderr, "\r\x1b[K");
        let _ = stderr.flush();
    });

    let out = work.await;
    done.store(true, Ordering::Release);
    let _ = spinner.await;
    out
}
