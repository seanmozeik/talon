use eyre::Result;
use serde::Serialize;
use std::io::{self, Write};
use talon_core::TalonEnvelope;

pub mod agent;

pub(super) fn emit_pretty(envelope: &TalonEnvelope) -> Result<()> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer_pretty(&mut lock, envelope)?;
    writeln!(lock)?;
    Ok(())
}

pub(super) fn emit_agent(envelope: &TalonEnvelope) -> Result<()> {
    agent::emit(envelope)
}

pub(super) fn emit_compact(value: &impl Serialize) -> Result<()> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, value)?;
    writeln!(lock)?;
    Ok(())
}
