//! Sample rate. The kernel audio driver owns the streaming interface, so the
//! rate is set by asking PipeWire to run its graph at that rate; the device
//! follows when the stream reopens.

use std::path::Path;
use std::process::{Command, Output};

/// Rates the iD2+ runs at. EQ and the compressor stop above 96 kHz.
pub const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
/// Prefer the system binary over whatever `PATH` finds first.
fn pw_metadata() -> &'static str {
    const SYSTEM: &str = "/usr/bin/pw-metadata";
    if Path::new(SYSTEM).exists() {
        SYSTEM
    } else {
        "pw-metadata"
    }
}

fn check(out: std::io::Result<Output>) -> Result<(), String> {
    let out = out.map_err(|e| format!("pw-metadata: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "pw-metadata: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Force the PipeWire graph rate without blocking the UI.
pub async fn force(hz: u32) -> Result<(), String> {
    check(
        tokio::process::Command::new(pw_metadata())
            .args(["-n", "settings", "0", "clock.force-rate", &hz.to_string()])
            .output()
            .await,
    )
}

/// Hand the graph rate back to PipeWire (0 lifts the force). Blocks briefly;
/// for shutdown only.
pub fn release() {
    let _ = check(
        Command::new(pw_metadata())
            .args(["-n", "settings", "0", "clock.force-rate", "0"])
            .output(),
    );
}
